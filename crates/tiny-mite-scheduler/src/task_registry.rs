//! Durable task registry with SQLite persistence.
//!
//! # Architecture
//!
//! The Task Registry is the single authoritative source for task state.
//! All task operations emit events through the EventBus. State transitions
//! are validated and protected with optimistic concurrency (version column).
//!
//! # State machine
//!
//! The canonical task lifecycle follows the `TaskStatus` enum from the
//! domain crate. Invalid transitions (e.g. COMPLETE → EXECUTING) are
//! rejected.
//!
//! # Crash recovery
//!
//! On startup, `recover_interrupted_tasks()` finds tasks that were in
//! an active state when the application stopped. These tasks are NOT
//! automatically re-executed; they are marked for recovery evaluation.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use tiny_mite_domain::{
    CorrelationId, DomainError, ErrorCategory, Priority, ProjectId, ResourceBudget, RetryPolicy,
    SecurityContext, TaskId, TaskStatus,
};
use tiny_mite_events::{EventBus, EventEnvelope};

// ── Task Registry error ──────────────────────────────────────────

/// Errors specific to task registry operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskRegistryError {
    /// The task does not exist.
    #[error("Task not found: {0}")]
    NotFound(TaskId),

    /// The task already exists (duplicate ID).
    #[error("Task already exists: {0}")]
    AlreadyExists(TaskId),

    /// The requested state transition is invalid.
    #[error("Invalid state transition: {current} → {requested} for task {task_id}")]
    InvalidTransition { current: TaskStatus, requested: TaskStatus, task_id: TaskId },

    /// The operation was attempted from a stale version (concurrent modification).
    #[error("Stale version for task {task_id}: expected v{expected}, found v{actual}")]
    StaleVersion { task_id: TaskId, expected: u32, actual: u32 },

    /// A database error occurred.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// The task is in a terminal state and cannot be modified.
    #[error("Task {0} is terminal ({1:?}) and cannot be modified")]
    TaskTerminal(TaskId, TaskStatus),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

// ── Task record ───────────────────────────────────────────────────

/// The authoritative record of a task in the system.
///
/// Contains task identity, lifecycle state, performance metadata,
/// cancellation information, and concurrency control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// Unique task identifier.
    pub id: TaskId,
    /// The project this task belongs to.
    pub project_id: Option<ProjectId>,
    /// Correlation ID for tracing the task across subsystems.
    pub correlation_id: CorrelationId,
    /// Optional parent task (for subtask decomposition).
    pub parent_id: Option<TaskId>,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Execution priority.
    pub priority: Priority,
    /// UTC timestamp when the task was created.
    pub created_at: DateTime<Utc>,
    /// UTC timestamp of the last mutation.
    pub updated_at: DateTime<Utc>,
    /// UTC timestamp when execution began (first attempt).
    pub started_at: Option<DateTime<Utc>>,
    /// UTC timestamp when the task reached a terminal state.
    pub completed_at: Option<DateTime<Utc>>,
    /// Number of execution attempts so far.
    pub attempt_count: u32,
    /// Maximum allowed attempts before the task is considered failed.
    pub max_attempts: u32,
    /// Resource budget for this task.
    pub resource_budget: ResourceBudget,
    /// Whether cancellation has been requested (the actual cancellation
    /// may happen on the next yield point).
    pub cancellation_requested: bool,
    /// Reason for cancellation, if any.
    pub cancellation_reason: Option<String>,
    /// Who or what requested cancellation.
    pub cancellation_source: Option<String>,
    /// When cancellation was requested or applied.
    pub cancelled_at: Option<DateTime<Utc>>,
    /// Last error, if the task failed or is blocked.
    pub error_message: Option<String>,
    /// Error category for classification.
    pub error_category: Option<String>,
    /// Whether the error is retryable.
    pub retry_policy: Option<String>,
    /// Whether the task can be resumed after interruption.
    pub is_resumable: bool,
    /// Concurrency control: monotonically incremented on each update.
    pub version: u32,
    /// Extensible JSON metadata.
    pub metadata: serde_json::Value,
}

impl TaskRecord {
    /// Create a new task record with sensible defaults.
    #[must_use]
    pub fn new(
        id: TaskId,
        project_id: Option<ProjectId>,
        correlation_id: CorrelationId,
        priority: Priority,
        resource_budget: ResourceBudget,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            project_id,
            correlation_id,
            parent_id: None,
            status: TaskStatus::New,
            priority,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
            attempt_count: 0,
            max_attempts: 3,
            resource_budget,
            cancellation_requested: false,
            cancellation_reason: None,
            cancellation_source: None,
            cancelled_at: None,
            error_message: None,
            error_category: None,
            retry_policy: None,
            is_resumable: true,
            version: 0,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// Returns `true` if the task can be safely modified.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    /// Returns `true` if the task was interrupted (active when app stopped).
    #[must_use]
    pub fn is_interrupted(&self) -> bool {
        self.status.is_active() && self.started_at.is_some()
    }
}

// ── State transition validation ──────────────────────────────────

/// Returns `Ok(())` if the transition from `current` to `next` is valid.
pub fn validate_transition(current: TaskStatus, next: TaskStatus) -> Result<(), TaskRegistryError> {
    use TaskStatus::*;

    let valid = match (current, next) {
        // Normal forward progression + shortcuts
        (New, Classifying) => true,
        (New, Executing) => true,
        (Classifying, Planning) => true,
        (Classifying, Executing) => true,
        (Planning, ContextPreparing) => true,
        (Planning, Executing) => true,
        (ContextPreparing, Executing) => true,
        (Executing, Verifying) => true,
        (Verifying, Reflecting) => true,
        (Verifying, Repairing) => true,
        (Reflecting, MemoryUpdate) => true,
        (MemoryUpdate, Complete) => true,
        // Repair loop: verify → repair → verify
        (Repairing, Verifying) => true,
        // Cancellation from any active state
        (s, Cancelled) if s.is_active() => true,
        // Completion from any active state (including New — pragmatic shortcut)
        (s, Complete) if s.is_active() => true,
        // Blocking from any active state
        (s, Blocked) if s.is_active() => true,
        // Unblocking
        (Blocked, Executing) => true,
        // Recovering interrupted tasks
        (s, New) if s.is_active() && s != New => true, // reset for recovery
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(TaskRegistryError::InvalidTransition {
            current,
            requested: next,
            task_id: TaskId::new(), // caller should provide
        })
    }
}

// ── Task registry query ──────────────────────────────────────────

/// Filter criteria for listing/querying tasks.
#[derive(Debug, Clone, Default)]
pub struct TaskQuery {
    /// Filter by status.
    pub status: Option<TaskStatus>,
    /// Filter by project.
    pub project_id: Option<ProjectId>,
    /// Filter by correlation ID.
    pub correlation_id: Option<CorrelationId>,
    /// Filter by priority.
    pub priority: Option<Priority>,
    /// Only return tasks that are candidates for recovery.
    pub interrupted_only: bool,
    /// Maximum results.
    pub limit: Option<usize>,
    /// Pagination offset.
    pub offset: Option<usize>,
}

// ── TaskRegistry trait ───────────────────────────────────────────

/// The authoritative interface for task lifecycle management.
#[async_trait::async_trait]
pub trait TaskRegistry: Send + Sync {
    /// Create a new task. Returns an error if the task ID already exists.
    async fn create(&self, task: &TaskRecord) -> Result<TaskRecord, TaskRegistryError>;

    /// Retrieve a task by ID.
    async fn get(&self, id: &TaskId) -> Result<Option<TaskRecord>, TaskRegistryError>;

    /// Transition a task to a new status (validates the transition).
    async fn transition(
        &self,
        id: &TaskId,
        expected_version: u32,
        new_status: TaskStatus,
    ) -> Result<TaskRecord, TaskRegistryError>;

    /// Update the task record (full write-back with version check).
    async fn update(
        &self,
        task: &TaskRecord,
        expected_version: u32,
    ) -> Result<TaskRecord, TaskRegistryError>;

    /// List tasks matching the given query.
    async fn query(&self, query: TaskQuery) -> Result<Vec<TaskRecord>, TaskRegistryError>;

    /// Request cancellation of a task.
    async fn request_cancellation(
        &self,
        id: &TaskId,
        reason: &str,
        source: &str,
    ) -> Result<TaskRecord, TaskRegistryError>;

    /// Mark a task as started (first execution attempt).
    async fn mark_started(&self, id: &TaskId) -> Result<TaskRecord, TaskRegistryError>;

    /// Mark a task as completed.
    async fn mark_completed(&self, id: &TaskId) -> Result<TaskRecord, TaskRegistryError>;

    /// Mark a task as failed with error details.
    async fn mark_failed(
        &self,
        id: &TaskId,
        error_msg: &str,
        error_category: &str,
    ) -> Result<TaskRecord, TaskRegistryError>;

    /// Find and return tasks that were interrupted (active when app stopped).
    async fn recover_interrupted_tasks(&self) -> Result<Vec<TaskRecord>, TaskRegistryError>;

    /// Delete/archive a task. Returns `Ok(true)` if the task existed.
    async fn delete(&self, id: &TaskId) -> Result<bool, TaskRegistryError>;

    /// Health check.
    async fn health_check(&self) -> Result<(), TaskRegistryError>;
}

// ── SQLite-backed TaskRegistry ───────────────────────────────────

/// SQLite-backed implementation of [`TaskRegistry`].
pub struct SqliteTaskRegistry {
    conn: Arc<Mutex<Connection>>,
    event_bus: EventBus,
}

impl SqliteTaskRegistry {
    const CURRENT_VERSION: u32 = 1;

    /// Create a new in-memory registry (primarily for testing).
    pub fn new_in_memory() -> Result<Self, TaskRegistryError> {
        let conn = Connection::open_in_memory()?;
        Self::migrate_blocking(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)), event_bus: EventBus::new() })
    }

    /// Create a new in-memory registry with a specific event bus.
    pub fn new_in_memory_with_bus(event_bus: EventBus) -> Result<Self, TaskRegistryError> {
        let conn = Connection::open_in_memory()?;
        Self::migrate_blocking(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)), event_bus })
    }

    /// Open a registry backed by a filesystem path.
    pub async fn open(
        path: impl AsRef<Path>,
        event_bus: EventBus,
    ) -> Result<Self, TaskRegistryError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;

        let reg = Self { conn: Arc::new(Mutex::new(conn)), event_bus };
        reg.migrate().await?;
        info!("TaskRegistry opened");
        Ok(reg)
    }

    /// Expose the event bus for subscription.
    #[must_use]
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    // ── Migrations ────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), TaskRegistryError> {
        let conn = self.conn.lock().await;
        Self::migrate_blocking(&conn)
    }

    fn migrate_blocking(conn: &Connection) -> Result<(), TaskRegistryError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _task_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let current: u32 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM _task_migrations", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        for v in (current + 1)..=Self::CURRENT_VERSION {
            match v {
                1 => Self::migrate_v1(conn)?,
                _ => {
                    return Err(TaskRegistryError::Database(rusqlite::Error::InvalidColumnName(
                        format!("Unknown migration v{v}"),
                    )));
                }
            }
            conn.execute("INSERT INTO _task_migrations (version) VALUES (?1)", params![v])?;
        }
        Ok(())
    }

    fn migrate_v1(conn: &Connection) -> Result<(), TaskRegistryError> {
        conn.execute_batch(
            "CREATE TABLE tasks (
                id TEXT NOT NULL PRIMARY KEY,
                project_id TEXT,
                correlation_id TEXT NOT NULL,
                parent_id TEXT,
                status TEXT NOT NULL DEFAULT 'NEW',
                priority TEXT NOT NULL DEFAULT 'normal',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                started_at TEXT,
                completed_at TEXT,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                max_attempts INTEGER NOT NULL DEFAULT 3,
                resource_budget_json TEXT NOT NULL DEFAULT '{}',
                cancellation_requested INTEGER NOT NULL DEFAULT 0,
                cancellation_reason TEXT,
                cancellation_source TEXT,
                cancelled_at TEXT,
                error_message TEXT,
                error_category TEXT,
                retry_policy TEXT,
                is_resumable INTEGER NOT NULL DEFAULT 1,
                version INTEGER NOT NULL DEFAULT 0,
                metadata_json TEXT NOT NULL DEFAULT '{}'
            );

            CREATE INDEX idx_tasks_status ON tasks(status);
            CREATE INDEX idx_tasks_project ON tasks(project_id);
            CREATE INDEX idx_tasks_correlation ON tasks(correlation_id);
            CREATE INDEX idx_tasks_started_status ON tasks(started_at, status);",
        )?;
        Ok(())
    }

    // ── Row mapping ───────────────────────────────────────────

    fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<TaskRecord> {
        let id_str: String = row.get("id")?;
        let id = TaskId::try_from(id_str.as_str())
            .map_err(|e| rusqlite::Error::InvalidColumnName(e.to_string()))?;

        let corr_str: String = row.get("correlation_id")?;
        let correlation_id = CorrelationId::try_from(corr_str.as_str())
            .map_err(|e| rusqlite::Error::InvalidColumnName(e.to_string()))?;

        let project_id_str: Option<String> = row.get("project_id")?;
        let project_id = project_id_str.and_then(|s| ProjectId::try_from(s.as_str()).ok());

        let parent_id_str: Option<String> = row.get("parent_id")?;
        let parent_id = parent_id_str.and_then(|s| TaskId::try_from(s.as_str()).ok());

        let status_str: String = row.get("status")?;
        let status = match status_str.as_str() {
            "NEW" => TaskStatus::New,
            "CLASSIFYING" => TaskStatus::Classifying,
            "PLANNING" => TaskStatus::Planning,
            "CONTEXT_PREPARING" => TaskStatus::ContextPreparing,
            "EXECUTING" => TaskStatus::Executing,
            "VERIFYING" => TaskStatus::Verifying,
            "REFLECTING" => TaskStatus::Reflecting,
            "MEMORY_UPDATE" => TaskStatus::MemoryUpdate,
            "REPAIRING" => TaskStatus::Repairing,
            "COMPLETE" => TaskStatus::Complete,
            "CANCELLED" => TaskStatus::Cancelled,
            "BLOCKED" => TaskStatus::Blocked,
            _ => TaskStatus::New,
        };

        let priority_str: String = row.get("priority")?;
        let priority = match priority_str.as_str() {
            "low" => Priority::Low,
            "high" => Priority::High,
            "critical" => Priority::Critical,
            _ => Priority::Normal,
        };

        let budget_json: String = row.get("resource_budget_json")?;
        let resource_budget: ResourceBudget =
            serde_json::from_str(&budget_json).unwrap_or_default();

        let metadata_json: String = row.get("metadata_json")?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata_json).unwrap_or_default();

        let parse_ts = |s: String| {
            DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).unwrap_or_default()
        };

        Ok(TaskRecord {
            id,
            project_id,
            correlation_id,
            parent_id,
            status,
            priority,
            created_at: row.get::<_, String>("created_at").map(parse_ts)?,
            updated_at: row.get::<_, String>("updated_at").map(parse_ts)?,
            started_at: row.get::<_, Option<String>>("started_at")?.map(parse_ts),
            completed_at: row.get::<_, Option<String>>("completed_at")?.map(parse_ts),
            attempt_count: row.get("attempt_count")?,
            max_attempts: row.get("max_attempts")?,
            resource_budget,
            cancellation_requested: row.get::<_, i32>("cancellation_requested")? != 0,
            cancellation_reason: row.get("cancellation_reason")?,
            cancellation_source: row.get("cancellation_source")?,
            cancelled_at: row.get::<_, Option<String>>("cancelled_at")?.map(parse_ts),
            error_message: row.get("error_message")?,
            error_category: row.get("error_category")?,
            retry_policy: row.get("retry_policy")?,
            is_resumable: row.get::<_, i32>("is_resumable")? != 0,
            version: row.get("version")?,
            metadata,
        })
    }

    // ── Event helpers ─────────────────────────────────────────

    async fn emit_event(
        &self,
        event_type: &'static str,
        task: &TaskRecord,
        previous_status: Option<TaskStatus>,
    ) {
        #[derive(Debug, Clone, Serialize)]
        struct TaskEvent {
            task_id: String,
            correlation_id: String,
            status: String,
            previous_status: Option<String>,
            priority: String,
            attempt: u32,
        }

        impl tiny_mite_events::Event for TaskEvent {
            fn event_type(&self) -> &'static str {
                "task.state_changed"
            }
        }

        // We need a dynamic event_type, so we wrap manually
        let payload = serde_json::to_value(TaskEvent {
            task_id: task.id.to_string(),
            correlation_id: task.correlation_id.to_string(),
            status: task.status.to_string(),
            previous_status: previous_status.map(|s| s.to_string()),
            priority: task.priority.to_string(),
            attempt: task.attempt_count,
        })
        .unwrap_or_default();

        let envelope = EventEnvelope {
            id: tiny_mite_domain::EventId::new(),
            event_type: event_type.to_owned(),
            version: 1,
            timestamp: Utc::now(),
            correlation_id: Some(task.correlation_id),
            causation_id: None,
            source: "task-registry".to_owned(),
            priority: task.priority,
            security: SecurityContext::default(),
            payload,
            payload_type: "task_registry::TaskEvent".to_owned(),
        };

        self.event_bus.publish(envelope).await;
    }
}

// ── TaskRegistry implementation ──────────────────────────────────

#[async_trait::async_trait]
impl TaskRegistry for SqliteTaskRegistry {
    async fn create(&self, task: &TaskRecord) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = task.id.to_string();
        let budget_json = serde_json::to_string(&task.resource_budget)?;
        let metadata_json = serde_json::to_string(&task.metadata)?;

        conn.execute(
            "INSERT INTO tasks (id, project_id, correlation_id, parent_id,
             status, priority, created_at, updated_at, started_at, completed_at,
             attempt_count, max_attempts, resource_budget_json,
             cancellation_requested, cancellation_reason, cancellation_source, cancelled_at,
             error_message, error_category, retry_policy, is_resumable, version, metadata_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
            params![
                id_str,
                task.project_id.as_ref().map(|p| p.to_string()),
                task.correlation_id.to_string(),
                task.parent_id.as_ref().map(|p| p.to_string()),
                task.status.to_string(),
                task.priority.to_string(),
                task.created_at.to_rfc3339(),
                task.updated_at.to_rfc3339(),
                task.started_at.as_ref().map(|t| t.to_rfc3339()),
                task.completed_at.as_ref().map(|t| t.to_rfc3339()),
                task.attempt_count,
                task.max_attempts,
                budget_json,
                task.cancellation_requested as i32,
                task.cancellation_reason,
                task.cancellation_source,
                task.cancelled_at.as_ref().map(|t| t.to_rfc3339()),
                task.error_message,
                task.error_category,
                task.retry_policy,
                task.is_resumable as i32,
                task.version,
                metadata_json,
            ],
        )?;

        debug!(task_id = %id_str, "Task created");
        drop(conn);
        self.emit_event("task.created", task, None).await;
        Ok(task.clone())
    }

    async fn get(&self, id: &TaskId) -> Result<Option<TaskRecord>, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let result = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?;
        Ok(result)
    }

    async fn transition(
        &self,
        id: &TaskId,
        expected_version: u32,
        new_status: TaskStatus,
    ) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();

        let current = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?
            .ok_or_else(|| TaskRegistryError::NotFound(*id))?;

        if current.version != expected_version {
            return Err(TaskRegistryError::StaleVersion {
                task_id: *id,
                expected: expected_version,
                actual: current.version,
            });
        }

        if current.is_terminal() {
            return Err(TaskRegistryError::TaskTerminal(*id, current.status));
        }

        // Validate transition
        if let Err(mut e) = validate_transition(current.status, new_status) {
            // Fix the task_id in the error
            if let TaskRegistryError::InvalidTransition { current: c, requested: r, .. } = &e {
                e = TaskRegistryError::InvalidTransition {
                    current: *c,
                    requested: *r,
                    task_id: *id,
                };
            }
            return Err(e);
        }

        let now = Utc::now();
        let new_version = current.version + 1;
        let mut updated = current.clone();
        updated.status = new_status;
        updated.updated_at = now;
        updated.version = new_version;

        if new_status == TaskStatus::Executing && updated.started_at.is_none() {
            updated.started_at = Some(now);
        }
        if new_status.is_terminal() && updated.completed_at.is_none() {
            updated.completed_at = Some(now);
        }
        if new_status == TaskStatus::Cancelled {
            updated.cancelled_at = Some(now);
        }

        conn.execute(
            "UPDATE tasks SET status=?1, updated_at=?2, version=?3,
             started_at=?4, completed_at=?5, cancelled_at=?6
             WHERE id=?7 AND version=?8",
            params![
                new_status.to_string(),
                now.to_rfc3339(),
                new_version,
                updated.started_at.as_ref().map(|t| t.to_rfc3339()),
                updated.completed_at.as_ref().map(|t| t.to_rfc3339()),
                updated.cancelled_at.as_ref().map(|t| t.to_rfc3339()),
                id_str,
                expected_version,
            ],
        )?;

        let previous = current.status;
        debug!(task_id = %id_str, status = %new_status, "Task transition");
        drop(conn);
        self.emit_event("task.state_changed", &updated, Some(previous)).await;
        Ok(updated)
    }

    async fn update(
        &self,
        task: &TaskRecord,
        expected_version: u32,
    ) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = task.id.to_string();

        let current = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?
            .ok_or_else(|| TaskRegistryError::NotFound(task.id))?;

        if current.version != expected_version {
            return Err(TaskRegistryError::StaleVersion {
                task_id: task.id,
                expected: expected_version,
                actual: current.version,
            });
        }

        let new_version = expected_version + 1;
        let now = Utc::now();
        let budget_json = serde_json::to_string(&task.resource_budget)?;
        let metadata_json = serde_json::to_string(&task.metadata)?;

        conn.execute(
            "UPDATE tasks SET
                project_id=?2, correlation_id=?3, parent_id=?4,
                status=?5, priority=?6, updated_at=?7,
                started_at=?8, completed_at=?9,
                attempt_count=?10, max_attempts=?11, resource_budget_json=?12,
                cancellation_requested=?13, cancellation_reason=?14,
                cancellation_source=?15, cancelled_at=?16,
                error_message=?17, error_category=?18, retry_policy=?19,
                is_resumable=?20, version=?21, metadata_json=?22
             WHERE id=?1",
            params![
                id_str,
                task.project_id.as_ref().map(|p| p.to_string()),
                task.correlation_id.to_string(),
                task.parent_id.as_ref().map(|p| p.to_string()),
                task.status.to_string(),
                task.priority.to_string(),
                now.to_rfc3339(),
                task.started_at.as_ref().map(|t| t.to_rfc3339()),
                task.completed_at.as_ref().map(|t| t.to_rfc3339()),
                task.attempt_count,
                task.max_attempts,
                budget_json,
                task.cancellation_requested as i32,
                task.cancellation_reason,
                task.cancellation_source,
                task.cancelled_at.as_ref().map(|t| t.to_rfc3339()),
                task.error_message,
                task.error_category,
                task.retry_policy,
                task.is_resumable as i32,
                new_version,
                metadata_json,
            ],
        )?;

        let mut updated = task.clone();
        updated.updated_at = now;
        updated.version = new_version;
        drop(conn);
        Ok(updated)
    }

    async fn query(&self, query: TaskQuery) -> Result<Vec<TaskRecord>, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref status) = query.status {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND status = ?{idx}"));
            param_values.push(Box::new(status.to_string()));
        }
        if let Some(ref proj) = query.project_id {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND project_id = ?{idx}"));
            param_values.push(Box::new(proj.to_string()));
        }
        if let Some(ref cid) = query.correlation_id {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND correlation_id = ?{idx}"));
            param_values.push(Box::new(cid.to_string()));
        }
        if let Some(ref priority) = query.priority {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND priority = ?{idx}"));
            param_values.push(Box::new(priority.to_string()));
        }
        if query.interrupted_only {
            sql.push_str(
                " AND started_at IS NOT NULL AND status NOT IN ('COMPLETE','CANCELLED','BLOCKED')",
            );
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = query.limit {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" LIMIT ?{idx}"));
            param_values.push(Box::new(limit as i64));
        }
        if let Some(offset) = query.offset {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" OFFSET ?{idx}"));
            param_values.push(Box::new(offset as i64));
        }

        let param_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref() as &dyn rusqlite::types::ToSql).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_slice.as_slice(), |row| Self::row_to_task(row))?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        Ok(tasks)
    }

    async fn request_cancellation(
        &self,
        id: &TaskId,
        reason: &str,
        source: &str,
    ) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let now = Utc::now();

        let current = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?
            .ok_or_else(|| TaskRegistryError::NotFound(*id))?;

        let new_version = current.version + 1;

        conn.execute(
            "UPDATE tasks SET
                cancellation_requested = 1,
                cancellation_reason = ?2,
                cancellation_source = ?3,
                cancelled_at = ?4,
                updated_at = ?5,
                version = ?6
             WHERE id = ?1",
            params![id_str, reason, source, now.to_rfc3339(), now.to_rfc3339(), new_version,],
        )?;

        let mut updated = current;
        updated.cancellation_requested = true;
        updated.cancellation_reason = Some(reason.to_owned());
        updated.cancellation_source = Some(source.to_owned());
        updated.cancelled_at = Some(now);
        updated.updated_at = now;
        updated.version = new_version;

        debug!(task_id = %id_str, "Cancellation requested");
        drop(conn);
        self.emit_event("task.cancel_requested", &updated, None).await;
        Ok(updated)
    }

    async fn mark_started(&self, id: &TaskId) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let now = Utc::now();

        let current = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?
            .ok_or_else(|| TaskRegistryError::NotFound(*id))?;

        let new_version = current.version + 1;
        let new_count = current.attempt_count + 1;

        conn.execute(
            "UPDATE tasks SET
                started_at = COALESCE(started_at, ?2),
                attempt_count = ?3,
                updated_at = ?4,
                version = ?5
             WHERE id = ?1",
            params![id_str, now.to_rfc3339(), new_count, now.to_rfc3339(), new_version,],
        )?;

        let mut updated = current;
        if updated.started_at.is_none() {
            updated.started_at = Some(now);
        }
        updated.attempt_count = new_count;
        updated.updated_at = now;
        updated.version = new_version;

        drop(conn);
        self.emit_event("task.started", &updated, None).await;
        Ok(updated)
    }

    async fn mark_completed(&self, id: &TaskId) -> Result<TaskRecord, TaskRegistryError> {
        let current = self.get(id).await?.ok_or_else(|| TaskRegistryError::NotFound(*id))?;
        self.transition(id, current.version, TaskStatus::Complete).await
    }

    async fn mark_failed(
        &self,
        id: &TaskId,
        error_msg: &str,
        error_category: &str,
    ) -> Result<TaskRecord, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let now = Utc::now();

        let current = conn
            .query_row("SELECT * FROM tasks WHERE id = ?1", params![id_str], |row| {
                Self::row_to_task(row)
            })
            .optional()?
            .ok_or_else(|| TaskRegistryError::NotFound(*id))?;

        let can_retry = current.attempt_count < current.max_attempts;
        let new_status = if can_retry { TaskStatus::Repairing } else { TaskStatus::Blocked };
        let retry_policy = if can_retry { "Retryable" } else { "NonRetryable" };
        let new_version = current.version + 1;

        conn.execute(
            "UPDATE tasks SET
                status = ?2,
                error_message = ?3,
                error_category = ?4,
                retry_policy = ?5,
                updated_at = ?6,
                completed_at = CASE WHEN ?7 THEN NULL ELSE COALESCE(completed_at, ?6) END,
                version = ?8
             WHERE id = ?1",
            params![
                id_str,
                new_status.to_string(),
                error_msg,
                error_category,
                retry_policy,
                now.to_rfc3339(),
                can_retry as i32,
                new_version,
            ],
        )?;

        let mut updated = current;
        updated.status = new_status;
        updated.error_message = Some(error_msg.to_owned());
        updated.error_category = Some(error_category.to_owned());
        updated.retry_policy = Some(retry_policy.to_owned());
        updated.updated_at = now;
        if !can_retry && updated.completed_at.is_none() {
            updated.completed_at = Some(now);
        }
        updated.version = new_version;

        debug!(task_id = %id_str, status = %new_status, "Task marked failed");
        drop(conn);
        self.emit_event("task.failed", &updated, None).await;
        Ok(updated)
    }

    async fn recover_interrupted_tasks(&self) -> Result<Vec<TaskRecord>, TaskRegistryError> {
        let tasks = self.query(TaskQuery { interrupted_only: true, ..Default::default() }).await?;

        for task in &tasks {
            let _ = self.emit_event("task.recovery_required", task, None).await;
        }

        if !tasks.is_empty() {
            info!(count = tasks.len(), "Found interrupted tasks requiring recovery evaluation");
        }

        Ok(tasks)
    }

    async fn delete(&self, id: &TaskId) -> Result<bool, TaskRegistryError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let count = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id_str])?;
        Ok(count > 0)
    }

    async fn health_check(&self) -> Result<(), TaskRegistryError> {
        let conn = self.conn.lock().await;
        conn.query_row("SELECT COUNT(*) FROM tasks", [], |_| Ok(()))?;
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use tiny_mite_domain::{CorrelationId, Priority, ProjectId, ResourceBudget, SecurityContext};

    fn make_task() -> TaskRecord {
        TaskRecord::new(
            TaskId::new(),
            None,
            CorrelationId::new(),
            Priority::Normal,
            ResourceBudget::default(),
        )
    }

    async fn new_registry() -> SqliteTaskRegistry {
        SqliteTaskRegistry::new_in_memory().expect("in-memory registry")
    }

    // ── Creation ──────────────────────────────────────────────

    #[tokio::test]
    async fn create_and_retrieve() {
        let reg = new_registry().await;
        let task = make_task();
        let created = reg.create(&task).await.expect("create");
        assert_eq!(created.id, task.id);

        let retrieved = reg.get(&task.id).await.expect("get").expect("present");
        assert_eq!(retrieved.status, TaskStatus::New);
    }

    #[tokio::test]
    async fn duplicate_create_fails() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("first create");
        let result = reg.create(&task).await;
        // This should fail because we used INSERT, not INSERT OR IGNORE
        assert!(result.is_err());
    }

    // ── State transitions ─────────────────────────────────────

    #[tokio::test]
    async fn valid_transition_succeeds() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        let updated =
            reg.transition(&task.id, 0, TaskStatus::Classifying).await.expect("transition");
        assert_eq!(updated.status, TaskStatus::Classifying);
        assert_eq!(updated.version, 1);
    }

    #[tokio::test]
    async fn invalid_transition_rejected() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        // Complete → Executing is invalid
        reg.transition(&task.id, 0, TaskStatus::Complete).await.expect("complete");
        let result = reg.transition(&task.id, 1, TaskStatus::Executing).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn terminal_state_protected() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");
        reg.transition(&task.id, 0, TaskStatus::Complete).await.expect("complete");

        let result = reg.transition(&task.id, 1, TaskStatus::Executing).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stale_version_rejected() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        // First transition succeeds (v0 → v1)
        reg.transition(&task.id, 0, TaskStatus::Classifying).await.expect("first transition");

        // Second transition with stale version (v0) should fail
        let result = reg.transition(&task.id, 0, TaskStatus::Planning).await;
        assert!(result.is_err());
    }

    // ── Cancellation ──────────────────────────────────────────

    #[tokio::test]
    async fn request_cancellation_sets_flags() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        let updated =
            reg.request_cancellation(&task.id, "user request", "user-1").await.expect("cancel");
        assert!(updated.cancellation_requested);
        assert_eq!(updated.cancellation_reason.as_deref(), Some("user request"));
        assert!(updated.cancelled_at.is_some());
    }

    #[tokio::test]
    async fn repeated_cancellation_request_is_safe() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        reg.request_cancellation(&task.id, "first", "src1").await.unwrap();
        reg.request_cancellation(&task.id, "second", "src2").await.unwrap();

        let current = reg.get(&task.id).await.expect("get").expect("present");
        // Second call should just update to the new values
        assert_eq!(current.cancellation_reason.as_deref(), Some("second"));
    }

    // ── Mark started / completed ──────────────────────────────

    #[tokio::test]
    async fn mark_started_increments_attempt() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        let started = reg.mark_started(&task.id).await.expect("started");
        assert_eq!(started.attempt_count, 1);
        assert!(started.started_at.is_some());
    }

    #[tokio::test]
    async fn mark_failed_with_retries() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");
        reg.mark_started(&task.id).await.expect("started");

        let failed = reg.mark_failed(&task.id, "test error", "Transient").await.expect("failed");
        assert_eq!(failed.status, TaskStatus::Repairing);
        assert_eq!(failed.error_message.as_deref(), Some("test error"));
        assert_eq!(failed.attempt_count, 1);
    }

    #[tokio::test]
    async fn mark_failed_exceeding_max_attempts_blocks() {
        let reg = new_registry().await;
        let mut task = make_task();
        task.max_attempts = 1;
        reg.create(&task).await.expect("create");
        reg.mark_started(&task.id).await.expect("started");

        let failed = reg.mark_failed(&task.id, "fatal", "Permanent").await.expect("failed");
        assert_eq!(failed.status, TaskStatus::Blocked);
        assert!(failed.completed_at.is_some());
    }

    // ── Recovery ──────────────────────────────────────────────

    #[tokio::test]
    async fn active_task_detected_as_interrupted() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");
        reg.transition(&task.id, 0, TaskStatus::Executing).await.expect("transition");
        reg.mark_started(&task.id).await.expect("started");

        let interrupted = reg.recover_interrupted_tasks().await.expect("recover");
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].id, task.id);
    }

    #[tokio::test]
    async fn completed_task_not_recoverable() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");
        reg.transition(&task.id, 0, TaskStatus::Executing).await.expect("execute");
        reg.transition(&task.id, 1, TaskStatus::Complete).await.expect("complete");

        let interrupted = reg.recover_interrupted_tasks().await.expect("recover");
        assert!(interrupted.is_empty());
    }

    #[tokio::test]
    async fn cancelled_task_not_incorrectly_resumed() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");
        reg.transition(&task.id, 0, TaskStatus::Executing).await.expect("execute");
        reg.transition(&task.id, 1, TaskStatus::Cancelled).await.expect("cancel");

        let interrupted = reg.recover_interrupted_tasks().await.expect("recover");
        assert!(interrupted.is_empty());
    }

    // ── Query ─────────────────────────────────────────────────

    #[tokio::test]
    async fn query_by_status() {
        let reg = new_registry().await;
        let t1 = make_task();
        reg.create(&t1).await.expect("create1");
        let t2 = make_task();
        reg.create(&t2).await.expect("create2");
        reg.transition(&t2.id, 0, TaskStatus::Complete).await.expect("complete");

        let completed = reg
            .query(TaskQuery { status: Some(TaskStatus::Complete), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(completed.len(), 1);
    }

    #[tokio::test]
    async fn query_with_pagination() {
        let reg = new_registry().await;
        for _ in 0..10 {
            reg.create(&make_task()).await.expect("create");
        }

        let page = reg
            .query(TaskQuery { limit: Some(3), offset: None, ..Default::default() })
            .await
            .expect("query");
        assert_eq!(page.len(), 3);
    }

    // ── Persistence ───────────────────────────────────────────

    #[tokio::test]
    async fn task_survives_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("tasks.db");

        let task = make_task();
        {
            let bus = EventBus::new();
            let reg = SqliteTaskRegistry::open(&db_path, bus).await.expect("open");
            reg.create(&task).await.expect("create");
            reg.transition(&task.id, 0, TaskStatus::Executing).await.expect("execute");
        }

        {
            let bus = EventBus::new();
            let reg = SqliteTaskRegistry::open(&db_path, bus).await.expect("reopen");
            let recovered = reg.get(&task.id).await.expect("get").expect("present");
            assert_eq!(recovered.status, TaskStatus::Executing);
        }
    }

    // ── Delete ─────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_removes_task() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        let deleted = reg.delete(&task.id).await.expect("delete");
        assert!(deleted);

        let result = reg.get(&task.id).await.expect("get");
        assert!(result.is_none());
    }

    // ── Health check ──────────────────────────────────────────

    #[tokio::test]
    async fn health_check_passes() {
        let reg = new_registry().await;
        reg.health_check().await.expect("healthy");
    }

    // ── Events ─────────────────────────────────────────────────

    #[tokio::test]
    async fn creation_emits_event() {
        let bus = EventBus::new();
        let reg = SqliteTaskRegistry::new_in_memory_with_bus(bus.clone()).unwrap();
        let task = make_task();

        let mut sub = bus
            .subscribe("task.", 16, tiny_mite_events::bus::OverflowPolicy::DropNewest)
            .await
            .expect("subscribe");

        reg.create(&task).await.expect("create");

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), sub.recv())
            .await
            .expect("timeout")
            .expect("event");
        assert_eq!(event.event_type, "task.created");
    }

    #[tokio::test]
    async fn state_transition_emits_event() {
        let bus = EventBus::new();
        let reg = SqliteTaskRegistry::new_in_memory_with_bus(bus.clone()).unwrap();
        let task = make_task();
        reg.create(&task).await.expect("create");

        let mut sub = bus
            .subscribe("task.state_changed", 16, tiny_mite_events::bus::OverflowPolicy::DropNewest)
            .await
            .expect("subscribe");

        // Skip the 'task.created' event
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), sub.recv()).await;

        // New → Complete is a valid pragmatic shortcut
        reg.transition(&task.id, 0, TaskStatus::Complete).await.expect("transition");

        let event = tokio::time::timeout(std::time::Duration::from_millis(100), sub.recv())
            .await
            .expect("timeout")
            .expect("event");
        assert_eq!(event.event_type, "task.state_changed");
    }

    // ── Concurrency ───────────────────────────────────────────

    #[tokio::test]
    async fn concurrent_transition_on_stale_version_fails() {
        let reg = new_registry().await;
        let task = make_task();
        reg.create(&task).await.expect("create");

        // Both try to transition from v0
        let result1 = reg.transition(&task.id, 0, TaskStatus::Classifying).await;
        let result2 = reg.transition(&task.id, 0, TaskStatus::Planning).await;

        // One should succeed, one should fail (stale version)
        let ok = result1.is_ok() && result2.is_err() || result1.is_err() && result2.is_ok();
        assert!(ok, "Expected exactly one success: r1={result1:?}, r2={result2:?}");
    }
}
