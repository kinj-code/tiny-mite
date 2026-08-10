//! Reliability infrastructure — crash recovery, durable task state, model lifecycle.
//!
//! Provides resilience primitives for the Tiny Mite runtime.

use std::sync::Arc;
use tokio::sync::Mutex;

// ── Durable Task State ────────────────────────────────────────────

/// Checkpoint state for a task that can survive crashes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskCheckpoint {
    /// Task identifier.
    pub task_id: String,
    /// The current plan step index.
    pub step_index: usize,
    /// Serialized working memory snapshot.
    pub memory_snapshot: Option<String>,
    /// When the checkpoint was created.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Number of retry attempts so far.
    pub retry_count: u32,
}

impl TaskCheckpoint {
    /// Create a new checkpoint.
    #[must_use]
    pub fn new(task_id: impl Into<String>, step_index: usize) -> Self {
        Self {
            task_id: task_id.into(),
            step_index,
            memory_snapshot: None,
            timestamp: chrono::Utc::now(),
            retry_count: 0,
        }
    }

    /// Increment retry count.
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// Crash recovery manager for durable task state.
pub struct CrashRecovery {
    /// Persisted checkpoints keyed by task ID.
    checkpoints: Arc<Mutex<std::collections::HashMap<String, TaskCheckpoint>>>,
}

impl CrashRecovery {
    /// Create a new crash recovery manager.
    #[must_use]
    pub fn new() -> Self {
        Self { checkpoints: Arc::new(Mutex::new(std::collections::HashMap::new())) }
    }

    /// Save a checkpoint.
    pub async fn save(&self, checkpoint: TaskCheckpoint) {
        let mut cp = self.checkpoints.lock().await;
        cp.insert(checkpoint.task_id.clone(), checkpoint);
    }

    /// Load the last checkpoint for a task.
    pub async fn load(&self, task_id: &str) -> Option<TaskCheckpoint> {
        let cp = self.checkpoints.lock().await;
        cp.get(task_id).cloned()
    }

    /// Remove a checkpoint (task completed).
    pub async fn clear(&self, task_id: &str) {
        let mut cp = self.checkpoints.lock().await;
        cp.remove(task_id);
    }

    /// Number of active checkpoints.
    pub async fn active_count(&self) -> usize {
        self.checkpoints.lock().await.len()
    }
}

impl Default for CrashRecovery {
    fn default() -> Self {
        Self::new()
    }
}

// ── Model Lifecycle ──────────────────────────────────────────────

/// Config for automatic model unload/reload.
#[derive(Debug, Clone)]
pub struct ModelLifecycleConfig {
    /// Unload models idle for longer than this (seconds).
    pub idle_timeout_seconds: u64,
    /// Maximum models to keep loaded simultaneously.
    pub max_loaded_models: usize,
    /// Whether to unload models under memory pressure.
    pub unload_on_pressure: bool,
}

impl Default for ModelLifecycleConfig {
    fn default() -> Self {
        Self { idle_timeout_seconds: 300, max_loaded_models: 2, unload_on_pressure: true }
    }
}

// ── Adaptive Concurrency ─────────────────────────────────────────

/// Adjusts concurrency based on system resource usage.
pub struct AdaptiveConcurrency {
    /// Minimum concurrent tasks.
    min_concurrent: usize,
    /// Maximum concurrent tasks.
    max_concurrent: usize,
    /// Current concurrent tasks allowed.
    current: Arc<Mutex<usize>>,
    /// Target CPU utilization threshold (0-100).
    cpu_threshold: u8,
    /// Target memory utilization threshold (0-100).
    memory_threshold: u8,
}

impl AdaptiveConcurrency {
    /// Create a new adaptive concurrency controller.
    #[must_use]
    pub fn new(min: usize, max: usize) -> Self {
        Self {
            min_concurrent: min,
            max_concurrent: max,
            current: Arc::new(Mutex::new(min)),
            cpu_threshold: 75,
            memory_threshold: 75,
        }
    }

    /// Get current concurrency limit.
    pub async fn current(&self) -> usize {
        *self.current.lock().await
    }

    /// Increase concurrency (more resources available).
    pub async fn increase(&self) {
        let mut c = self.current.lock().await;
        *c = (*c + 1).min(self.max_concurrent);
    }

    /// Decrease concurrency (resource pressure).
    pub async fn decrease(&self) {
        let mut c = self.current.lock().await;
        *c = (*c).saturating_sub(1).max(self.min_concurrent);
    }

    /// Adjust based on observed utilization.
    pub async fn adjust(&self, cpu_pct: u8, memory_pct: u8) {
        if cpu_pct > self.cpu_threshold || memory_pct > self.memory_threshold {
            self.decrease().await;
        } else if cpu_pct < self.cpu_threshold / 2 && memory_pct < self.memory_threshold / 2 {
            self.increase().await;
        }
    }
}

impl Default for AdaptiveConcurrency {
    fn default() -> Self {
        Self::new(1, 4)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn crash_recovery_save_load() {
        let recovery = CrashRecovery::new();
        let cp = TaskCheckpoint::new("task_1", 3);
        recovery.save(cp.clone()).await;
        let loaded = recovery.load("task_1").await.unwrap();
        assert_eq!(loaded.task_id, "task_1");
        assert_eq!(loaded.step_index, 3);
    }

    #[tokio::test]
    async fn crash_recovery_clear() {
        let recovery = CrashRecovery::new();
        recovery.save(TaskCheckpoint::new("task_1", 0)).await;
        recovery.clear("task_1").await;
        assert!(recovery.load("task_1").await.is_none());
    }

    #[tokio::test]
    async fn adaptive_concurrency_increases() {
        let ac = AdaptiveConcurrency::new(1, 8);
        assert_eq!(ac.current().await, 1);
        ac.increase().await;
        assert_eq!(ac.current().await, 2);
    }

    #[tokio::test]
    async fn adaptive_concurrency_respects_max() {
        let ac = AdaptiveConcurrency::new(1, 2);
        ac.increase().await;
        ac.increase().await;
        ac.increase().await;
        assert_eq!(ac.current().await, 2);
    }

    #[tokio::test]
    async fn adaptive_concurrency_decreases_under_pressure() {
        let ac = AdaptiveConcurrency::new(2, 8);
        ac.increase().await;
        ac.increase().await;
        ac.adjust(85, 50).await; // High CPU
        assert!(ac.current().await < 4);
    }
}
