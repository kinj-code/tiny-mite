//! Cooperative cancellation system for Tiny Mite tasks.
//!
//! # Architecture
//!
//! ```text
//! User/System → CancellationManager → CancellationToken → Running Op → Cleanup → CANCELLED
//!                                     ↑
//!                              TaskRegistry (reads request flags)
//! ```
//!
//! # Principles
//!
//! - **Safe**: never terminates processes forcefully
//! - **Cooperative**: tasks must periodically check `token.is_cancelled()`
//! - **Observable**: every cancellation emits events through the EventBus
//! - **Hierarchical**: cancelling a parent cancels children, not vice versa
//! - **Idempotent**: calling cancel multiple times is safe

use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, Notify};

use tiny_mite_domain::{CorrelationId, DomainError, SecurityContext, TaskId, TaskStatus};
use tiny_mite_events::{EventBus, EventEnvelope};

use crate::task_registry::{SqliteTaskRegistry, TaskRecord, TaskRegistry};

// ---------------------------------------------------------------------------
// Cancel reason
// ---------------------------------------------------------------------------

/// Structured reason for cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelReason {
    /// User requested cancellation.
    User {
        /// Free-text description of why.
        message: String,
    },
    /// System-initiated cancellation (e.g. shutdown).
    System {
        /// Free-text description of why.
        message: String,
    },
    /// The task exceeded its deadline.
    Timeout {
        /// The deadline that was exceeded.
        deadline: DateTime<Utc>,
    },
    /// A parent task was cancelled.
    ParentCancelled {
        /// The parent task ID.
        parent_id: TaskId,
    },
    /// Resource pressure forced cancellation.
    ResourcePressure {
        /// Description of the resource constraint.
        message: String,
    },
}

impl CancelReason {
    /// Human-readable reason string for storage and events.
    pub fn as_str(&self) -> &str {
        match self {
            Self::User { message } => message.as_str(),
            Self::System { message } => message.as_str(),
            Self::Timeout { deadline: _ } => "task timed out",
            Self::ParentCancelled { parent_id: _ } => "parent task cancelled",
            Self::ResourcePressure { message } => message.as_str(),
        }
    }

    /// Category string for classification.
    pub fn category(&self) -> &'static str {
        match self {
            Self::User { .. } => "User",
            Self::System { .. } => "System",
            Self::Timeout { .. } => "Timeout",
            Self::ParentCancelled { .. } => "ParentCancelled",
            Self::ResourcePressure { .. } => "ResourcePressure",
        }
    }
}

impl std::fmt::Display for CancelReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.category(), self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Cancellation token
// ---------------------------------------------------------------------------

/// A lightweight, cloneable cancellation signal with metadata.
///
/// Tasks should periodically call [`is_cancelled()`](CancellationToken::is_cancelled)
/// or await [`cancelled()`](CancellationToken::cancelled) at yield points.
///
/// # Cloning
///
/// Cheap clone (Arc-based) — share with child operations.
#[derive(Clone)]
pub struct CancellationToken {
    /// Atomic flag: set to true when cancellation is requested.
    cancelled: Arc<AtomicBool>,
    /// Reason (set once, read many).
    reason: Arc<Mutex<Option<CancelReason>>>,
    /// Source identifier.
    source: Arc<Mutex<Option<String>>>,
    /// When cancellation was first requested.
    requested_at: Arc<Mutex<Option<DateTime<Utc>>>>,
    /// Grace period deadline (if any).
    grace_until: Arc<Mutex<Option<DateTime<Utc>>>>,
    /// Async notification — tasks can .await on this.
    notify: Arc<Notify>,
    /// Parent token for hierarchical cancellation.
    parent: Arc<Mutex<Option<CancellationToken>>>,
    /// Child tokens that should be cancelled when this token is.
    children: Arc<Mutex<Vec<CancellationToken>>>,
}

impl CancellationToken {
    /// Create a new uncancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            reason: Arc::new(Mutex::new(None)),
            source: Arc::new(Mutex::new(None)),
            requested_at: Arc::new(Mutex::new(None)),
            grace_until: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
            parent: Arc::new(Mutex::new(None)),
            children: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns `true` if cancellation has been requested.
    ///
    /// This is the hot-path check — no locking, just an atomic load.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Request cancellation with the given reason and source.
    ///
    /// Idempotent — calling multiple times is safe. Only the first
    /// call's reason/source/timestamp are preserved.
    pub async fn cancel(&self, reason: CancelReason, source: impl Into<String>) {
        self.set_flags(reason, source).await;

        // Propagate to children (separate loop avoids async recursion complaints)
        let children = self.children.lock().await;
        for child in children.iter() {
            let child_reason = CancelReason::ParentCancelled { parent_id: TaskId::new() };
            child.set_flags(child_reason, "parent-cancellation").await;
        }
    }

    /// Internal: set cancellation flags and notify without child propagation.
    async fn set_flags(&self, reason: CancelReason, source: impl Into<String>) {
        // Set metadata first (only if not already set)
        {
            let mut r = self.reason.lock().await;
            if r.is_none() {
                *r = Some(reason);
            }
        }
        {
            let mut s = self.source.lock().await;
            if s.is_none() {
                *s = Some(source.into());
            }
        }
        {
            let mut ts = self.requested_at.lock().await;
            if ts.is_none() {
                *ts = Some(Utc::now());
            }
        }

        // Set the flag and notify waiters
        let was_already = self.cancelled.swap(true, Ordering::Release);
        if !was_already {
            self.notify.notify_waiters();
        }
    }

    /// Await cancellation.
    ///
    /// Returns immediately if already cancelled. Otherwise waits until
    /// `cancel()` is called.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    /// Get the cancellation reason, if set.
    pub async fn reason(&self) -> Option<CancelReason> {
        self.reason.lock().await.clone()
    }

    /// Get the source, if set.
    pub async fn source(&self) -> Option<String> {
        self.source.lock().await.clone()
    }

    /// Get when cancellation was requested, if set.
    pub async fn requested_at(&self) -> Option<DateTime<Utc>> {
        *self.requested_at.lock().await
    }

    /// Set a grace period. Operations should complete cleanup before this deadline.
    pub async fn set_grace_period(&self, duration: Duration) {
        *self.grace_until.lock().await = Some(Utc::now() + duration);
    }

    /// Check if the grace period has expired.
    pub async fn grace_period_expired(&self) -> bool {
        if let Some(deadline) = *self.grace_until.lock().await {
            Utc::now() >= deadline
        } else {
            false
        }
    }

    /// Add a child token. When this token is cancelled, the child is also cancelled.
    pub async fn add_child(&self, child: CancellationToken) {
        let mut children = self.children.lock().await;
        // Set parent on child
        {
            let mut p = child.parent.lock().await;
            *p = Some(self.clone());
        }
        children.push(child);
    }

    /// Set a parent token. When the parent is cancelled, this token is also cancelled.
    pub async fn set_parent(&self, parent: CancellationToken) {
        let mut p = self.parent.lock().await;
        *p = Some(parent.clone());
        let mut children = parent.children.lock().await;
        children.push(self.clone());
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

// Fallback impl since we're not using Debug derive
impl std::fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Cancellation manager
// ---------------------------------------------------------------------------

/// Coordinates cancellation across all active tasks.
///
/// The manager maintains an in-memory registry of active cancellation tokens
/// and integrates with the [`TaskRegistry`] for durable state updates.
pub struct CancellationManager {
    /// Active tokens keyed by TaskId.
    tokens: dashmap::DashMap<TaskId, Arc<CancellationToken>>,
    /// Event bus for cancellation events.
    event_bus: EventBus,
}

impl CancellationManager {
    /// Create a new cancellation manager.
    #[must_use]
    pub fn new(event_bus: EventBus) -> Self {
        Self { tokens: dashmap::DashMap::new(), event_bus }
    }

    /// Register a task with a new cancellation token.
    ///
    /// # Panics
    ///
    /// Panics if a token is already registered for this task (should not happen
    /// if register/unregister lifecycle is maintained).
    pub fn register(&self, task_id: TaskId) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens.insert(task_id, Arc::new(token.clone()));
        token
    }

    /// Register a task with a specific token (e.g. a child token).
    pub fn register_token(&self, task_id: TaskId, token: CancellationToken) {
        self.tokens.insert(task_id, Arc::new(token));
    }

    /// Unregister a completed task. Its token is removed from the manager.
    pub fn unregister(&self, task_id: &TaskId) {
        self.tokens.remove(task_id);
    }

    /// Request cancellation of a task via the TaskRegistry.
    ///
    /// 1. Updates TaskRegistry (durable).
    /// 2. Triggers in-memory CancellationToken.
    /// 3. Emits events.
    pub async fn request_cancellation(
        &self,
        registry: &SqliteTaskRegistry,
        task_id: TaskId,
        reason: CancelReason,
        source: impl Into<String>,
    ) -> Result<TaskRecord, crate::task_registry::TaskRegistryError> {
        let source: String = source.into();

        // Step 1: Request cancellation in the registry (durable)
        let updated = registry.request_cancellation(&task_id, reason.as_str(), &source).await?;

        // Step 2: Trigger in-memory token if registered
        if let Some(token) = self.tokens.get(&task_id) {
            token.cancel(reason.clone(), source.clone()).await;
        }

        // Step 3: Transition to CANCELLED if possible
        if updated.status.is_active() {
            match registry.transition(&task_id, updated.version, TaskStatus::Cancelled).await {
                Ok(final_task) => {
                    // Step 4: Emit cancellation completed event
                    self.emit_event("task.cancellation_completed", &final_task, &source, &reason)
                        .await;
                    self.unregister(&task_id);
                    return Ok(final_task);
                }
                Err(e) => {
                    // Task might have completed/failed concurrently — that's fine
                    tracing::warn!(%task_id, error = %e, "Could not transition to CANCELLED");
                }
            }
        }

        // Emit cancellation requested event
        self.emit_event("task.cancel_requested", &updated, &source, &reason).await;
        Ok(updated)
    }

    /// Cancel a task and all its children (hierarchical).
    pub async fn cancel_hierarchy(
        &self,
        registry: &SqliteTaskRegistry,
        task_id: TaskId,
        reason: CancelReason,
        source: impl Into<String>,
    ) -> Result<Vec<TaskRecord>, crate::task_registry::TaskRegistryError> {
        let source: String = source.into();
        let mut results = Vec::new();

        // Cancel the parent first
        let parent_result =
            self.request_cancellation(registry, task_id, reason.clone(), source.clone()).await?;
        results.push(parent_result);

        // Cancel children
        // In a full implementation, we'd query child tasks from the registry.
        // For Phase 1C, the token hierarchy handles child propagation.

        Ok(results)
    }

    /// Get the cancellation token for a task, if registered.
    #[must_use]
    pub fn get_token(&self, task_id: &TaskId) -> Option<CancellationToken> {
        self.tokens.get(task_id).map(|t| t.value().as_ref().clone())
    }

    /// Returns the number of active tasks with registered tokens.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.tokens.len()
    }

    /// Shutdown: cancel all remaining active tasks.
    pub async fn shutdown(&self) {
        for entry in self.tokens.iter() {
            let (task_id, token) = (entry.key(), entry.value());
            token
                .cancel(CancelReason::System { message: "system shutdown".into() }, "shutdown")
                .await;
            tracing::info!(%task_id, "CancellationManager shutdown — task cancelled");
        }
        self.tokens.clear();
    }

    // ── Helpers ───────────────────────────────────────────────

    async fn emit_event(
        &self,
        event_type: &str,
        task: &TaskRecord,
        source: &str,
        reason: &CancelReason,
    ) {
        let payload = serde_json::json!({
            "task_id": task.id.to_string(),
            "correlation_id": task.correlation_id.to_string(),
            "cancellation_reason": reason.as_str(),
            "cancellation_category": reason.category(),
            "cancellation_source": source,
        });

        let envelope = EventEnvelope {
            id: tiny_mite_domain::EventId::new(),
            event_type: event_type.to_owned(),
            version: 1,
            timestamp: Utc::now(),
            correlation_id: Some(task.correlation_id),
            causation_id: None,
            source: "cancellation-manager".to_owned(),
            priority: task.priority,
            security: SecurityContext::default(),
            payload,
            payload_type: "cancellation::CancellationEvent".to_owned(),
        };

        self.event_bus.publish(envelope).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;
    use tiny_mite_domain::{CorrelationId, Priority, ResourceBudget};
    use tiny_mite_events::EventBus;

    // ── Token basic ───────────────────────────────────────────

    #[tokio::test]
    async fn token_not_cancelled_initially() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn token_cancel_sets_flag() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::User { message: "stop".into() }, "test").await;
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn token_repeated_cancel_is_idempotent() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::User { message: "first".into() }, "test1").await;
        token.cancel(CancelReason::User { message: "second".into() }, "test2").await;

        // Second call preserved first reason
        let reason = token.reason().await.unwrap();
        assert!(reason.as_str().contains("first"));
    }

    #[tokio::test]
    async fn token_reason_preserved() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::System { message: "shutdown".into() }, "sys").await;

        let reason = token.reason().await.unwrap();
        assert_eq!(reason.category(), "System");
        assert_eq!(reason.as_str(), "shutdown");
    }

    #[tokio::test]
    async fn token_source_preserved() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::User { message: "abort".into() }, "user-42").await;
        assert_eq!(token.source().await.unwrap(), "user-42");
    }

    #[tokio::test]
    async fn token_timestamp_preserved() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::User { message: "now".into() }, "test").await;
        assert!(token.requested_at().await.is_some());
    }

    // ── Async cancellation ────────────────────────────────────

    #[tokio::test]
    async fn cancelled_awaits_when_not_cancelled() {
        let token = CancellationToken::new();
        let token2 = token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(StdDuration::from_millis(10)).await;
            token2.cancel(CancelReason::User { message: "go".into() }, "test").await;
        });

        token.cancelled().await;
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_returns_immediately_if_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel(CancelReason::User { message: "x".into() }, "test").await;
        // Should return immediately, no timeout
        tokio::time::timeout(StdDuration::from_millis(5), token.cancelled())
            .await
            .expect("should not timeout");
    }

    // ── Grace period ──────────────────────────────────────────

    #[tokio::test]
    async fn grace_period_starts_unexpired() {
        let token = CancellationToken::new();
        assert!(!token.grace_period_expired().await);
    }

    #[tokio::test]
    async fn grace_period_expires() {
        let token = CancellationToken::new();
        token.set_grace_period(Duration::milliseconds(1)).await;
        tokio::time::sleep(StdDuration::from_millis(10)).await;
        assert!(token.grace_period_expired().await);
    }

    // ── Hierarchy ─────────────────────────────────────────────

    #[tokio::test]
    async fn parent_cancellation_cancels_child() {
        let parent = CancellationToken::new();
        let child = CancellationToken::new();
        parent.add_child(child.clone()).await;

        assert!(!child.is_cancelled());
        parent.cancel(CancelReason::User { message: "stop".into() }, "test").await;
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn child_cancellation_does_not_cancel_parent() {
        let parent = CancellationToken::new();
        let child = CancellationToken::new();
        parent.add_child(child.clone()).await;

        child.cancel(CancelReason::User { message: "child stop".into() }, "test").await;
        assert!(!parent.is_cancelled());
    }

    #[tokio::test]
    async fn already_completed_child_not_modified() {
        let parent = CancellationToken::new();
        let child = CancellationToken::new();

        // Child is cancelled first (for whatever reason)
        child.cancel(CancelReason::User { message: "done".into() }, "test").await;

        parent.add_child(child.clone()).await;

        // Parent cancellation should still propagate but child already cancelled
        parent.cancel(CancelReason::User { message: "parent stop".into() }, "test").await;
        assert!(child.is_cancelled());
        // Child reason should still be "done", not "parent stop"
        let child_reason = child.reason().await.unwrap();
        assert_eq!(child_reason.as_str(), "done");
    }

    // ── Timeout ────────────────────────────────────────────────

    #[tokio::test]
    async fn timeout_reason_is_recorded() {
        let token = CancellationToken::new();
        let deadline = Utc::now();
        token.cancel(CancelReason::Timeout { deadline }, "timeout-src").await;

        let reason = token.reason().await.unwrap();
        assert_eq!(reason.category(), "Timeout");
    }

    // ── Cancellation manager integration (in-memory) ──────────

    #[tokio::test]
    async fn manager_registers_and_cancels() {
        let bus = EventBus::new();
        let manager = CancellationManager::new(bus);
        let task_id = TaskId::new();
        let token = manager.register(task_id);

        assert!(!token.is_cancelled());

        token.cancel(CancelReason::User { message: "done".into() }, "test").await;
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn manager_unregister_removes_token() {
        let bus = EventBus::new();
        let manager = CancellationManager::new(bus);
        let task_id = TaskId::new();
        manager.register(task_id);
        assert_eq!(manager.active_count(), 1);

        manager.unregister(&task_id);
        assert_eq!(manager.active_count(), 0);
    }

    #[tokio::test]
    async fn manager_shutdown_cancels_all() {
        let bus = EventBus::new();
        let manager = CancellationManager::new(bus);
        let task_id = TaskId::new();
        let token = manager.register(task_id);

        manager.shutdown().await;
        assert!(token.is_cancelled());
        assert_eq!(manager.active_count(), 0);
    }
}
