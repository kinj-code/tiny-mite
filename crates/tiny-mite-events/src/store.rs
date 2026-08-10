//! Durable event persistence backed by SQLite.
//!
//! # Architecture
//!
//! ```text
//! Event → EventEnvelope → EventStore (SQLite) → EventBus → Subscribers
//!                                                     ↓
//!                                              mark_processed (checkpoint)
//! ```
//!
//! The `EventStore` is **persistence**. The `EventBus` is **distribution**.
//! They cooperate but are separate abstractions.
//!
//! # Delivery guarantees
//!
//! **At-least-once delivery** combined with **idempotent consumers**.
//! Duplicate event IDs are silently accepted (INSERT OR IGNORE).
//!
//! # Security
//!
//! - Payloads are stored as JSON text in SQLite (no encryption yet).
//! - Replay does NOT bypass authorization — replay produces `EventEnvelope`
//!   values that must pass through the normal EventBus and tool gateway.
//! - Sensitive fields in payloads are the responsibility of event producers
//!   to handle via `SecretString` or equivalent.
//!
//! # Migrations
//!
//! Schema versions are tracked in the `_migrations` table. Migrations are
//! applied in order during `SqliteEventStore::open`.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use tiny_mite_domain::{
    CorrelationId, DomainError, ErrorCategory, EventId, Priority, SecurityContext,
};

use crate::envelope::EventEnvelope;

// ---------------------------------------------------------------------------
// Store error
// ---------------------------------------------------------------------------

/// Errors that can occur during event store operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The database connection or schema is invalid.
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// A serialization or deserialization error occurred.
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),

    /// The event payload is incompatible with the expected schema.
    #[error("Incompatible event version: expected {expected}, got {actual}")]
    IncompatibleVersion { expected: u32, actual: u32 },

    /// A required migration was not applied.
    #[error("Missing migration: version {0}")]
    MissingMigration(u32),

    /// The store has been closed.
    #[error("Event store is closed")]
    Closed,
}

impl From<StoreError> for DomainError {
    fn from(e: StoreError) -> Self {
        DomainError::permanent(format!("Event store error: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Checkpoint
// ---------------------------------------------------------------------------

/// A consumer's position in the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The ID of the last successfully processed event.
    pub last_event_id: String,
    /// The timestamp of that event.
    pub last_timestamp: DateTime<Utc>,
    /// When the checkpoint was recorded.
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

/// Filter criteria for querying events.
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    /// Filter by event type prefix (e.g. `"task."`).
    pub event_type: Option<String>,
    /// Filter by correlation ID.
    pub correlation_id: Option<CorrelationId>,
    /// Filter by causation ID.
    pub causation_id: Option<EventId>,
    /// Filter by source component.
    pub source: Option<String>,
    /// Earliest timestamp (inclusive).
    pub from_timestamp: Option<DateTime<Utc>>,
    /// Latest timestamp (inclusive).
    pub to_timestamp: Option<DateTime<Utc>>,
    /// Maximum number of events to return.
    pub limit: Option<usize>,
    /// Number of events to skip (for pagination).
    pub offset: Option<usize>,
}

/// Filter criteria for replaying events.
#[derive(Debug, Clone)]
pub struct ReplayFilter {
    /// Replay events starting after this checkpoint.
    pub after_checkpoint: Option<Checkpoint>,
    /// Replay events after this event ID (for a specific consumer).
    pub after_event_id: Option<EventId>,
    /// Restrict to specific event type prefix.
    pub event_type: Option<String>,
    /// Restrict to a specific time range.
    pub from_timestamp: Option<DateTime<Utc>>,
    pub to_timestamp: Option<DateTime<Utc>>,
    /// Maximum events to replay.
    pub limit: Option<usize>,
}

/// Filter for pruning old events.
#[derive(Debug, Clone)]
pub struct PruneFilter {
    /// Prune events older than this timestamp.
    pub before_timestamp: DateTime<Utc>,
    /// Optional event type to restrict pruning.
    pub event_type: Option<String>,
}

// ---------------------------------------------------------------------------
// EventStore trait
// ---------------------------------------------------------------------------

/// Abstraction over durable event persistence.
///
/// # At-least-once semantics
///
/// - `append`: idempotent — duplicate event IDs are silently accepted.
/// - `replay`: returns events; consumers are responsible for idempotent processing.
/// - `mark_processed`: records a consumer's checkpoint.
#[async_trait::async_trait]
pub trait EventStore: Send + Sync {
    /// Persist a single event. Idempotent for duplicate IDs.
    async fn append(&self, envelope: &EventEnvelope) -> Result<(), StoreError>;

    /// Persist a batch of events atomically.
    async fn append_batch(&self, envelopes: &[EventEnvelope]) -> Result<(), StoreError>;

    /// Retrieve an event by ID.
    async fn get(&self, id: &EventId) -> Result<Option<EventEnvelope>, StoreError>;

    /// Query events matching the given filter.
    async fn query(&self, filter: QueryFilter) -> Result<Vec<EventEnvelope>, StoreError>;

    /// Replay events matching the given filter.
    async fn replay(&self, filter: ReplayFilter) -> Result<Vec<EventEnvelope>, StoreError>;

    /// Mark an event as processed by a consumer (checkpoint).
    async fn mark_processed(&self, consumer_id: &str, event_id: &EventId)
    -> Result<(), StoreError>;

    /// Get the last checkpoint for a consumer.
    async fn get_checkpoint(&self, consumer_id: &str) -> Result<Option<Checkpoint>, StoreError>;

    /// Prune events matching the filter. Returns count of pruned events.
    async fn prune(&self, filter: PruneFilter) -> Result<usize, StoreError>;

    /// Verify the store is operational.
    async fn health_check(&self) -> Result<(), StoreError>;
}

// ---------------------------------------------------------------------------
// SQLite-backed EventStore
// ---------------------------------------------------------------------------

/// SQLite-backed implementation of [`EventStore`].
///
/// Uses WAL mode for concurrent reads. All write operations are serialized
/// through a mutex (appropriate for a local-first application).
pub struct SqliteEventStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteEventStore {
    // ── Schema ────────────────────────────────────────────────

    /// Current schema version.
    const CURRENT_VERSION: u32 = 1;

    /// Create a new in-memory store (primarily for testing).
    pub fn new_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        // Migrate before wrapping in Arc to avoid borrow issues
        Self::migrate_blocking(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Open a store backed by a filesystem path.
    ///
    /// Creates the database file if it doesn't exist. Applies all pending
    /// migrations automatically.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        // Enable WAL mode for better concurrent read performance
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;

        let store = Self { conn: Arc::new(Mutex::new(conn)) };
        store.migrate().await?;
        info!("EventStore opened with {} migrations applied", Self::CURRENT_VERSION);
        Ok(store)
    }

    // ── Migrations ────────────────────────────────────────────

    /// Apply all pending migrations.
    async fn migrate(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().await;
        Self::migrate_blocking(&conn)
    }

    fn migrate_blocking(conn: &Connection) -> Result<(), StoreError> {
        // Ensure migrations table exists
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let current: u32 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM _migrations", [], |row| row.get(0))
            .unwrap_or(0);

        for v in (current + 1)..=Self::CURRENT_VERSION {
            debug!("Applying migration v{v}");
            match v {
                1 => Self::migrate_v1(conn)?,
                _ => return Err(StoreError::MissingMigration(v)),
            }
            conn.execute("INSERT INTO _migrations (version) VALUES (?1)", params![v])?;
            info!("Migration v{v} applied");
        }

        Ok(())
    }

    /// Migration v1: create the events and checkpoints tables.
    fn migrate_v1(conn: &Connection) -> Result<(), StoreError> {
        conn.execute_batch(
            "CREATE TABLE events (
                id TEXT NOT NULL PRIMARY KEY,
                event_type TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                timestamp TEXT NOT NULL,
                correlation_id TEXT,
                causation_id TEXT,
                source TEXT NOT NULL,
                priority TEXT NOT NULL DEFAULT 'normal',
                security_subject TEXT NOT NULL DEFAULT 'user',
                security_scope TEXT NOT NULL DEFAULT 'project',
                payload_json TEXT NOT NULL,
                payload_type TEXT NOT NULL,
                persisted_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE checkpoints (
                consumer_id TEXT NOT NULL,
                last_event_id TEXT NOT NULL,
                last_timestamp TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (consumer_id)
            );

            CREATE INDEX idx_events_type ON events(event_type);
            CREATE INDEX idx_events_correlation ON events(correlation_id);
            CREATE INDEX idx_events_timestamp ON events(timestamp);
            CREATE INDEX idx_events_source ON events(source);",
        )?;
        Ok(())
    }

    // ── Row helpers ───────────────────────────────────────────

    fn row_to_envelope(row: &rusqlite::Row) -> rusqlite::Result<EventEnvelope> {
        let id_str: String = row.get("id")?;
        let id = EventId::try_from(id_str.as_str())
            .map_err(|e| rusqlite::Error::InvalidColumnName(e.to_string()))?;

        let corr_str: Option<String> = row.get("correlation_id")?;
        let correlation_id = corr_str.and_then(|s| CorrelationId::try_from(s.as_str()).ok());

        let cause_str: Option<String> = row.get("causation_id")?;
        let causation_id = cause_str.and_then(|s| EventId::try_from(s.as_str()).ok());

        let payload_json: String = row.get("payload_json")?;
        let payload: serde_json::Value = serde_json::from_str(&payload_json)
            .map_err(|e| rusqlite::Error::InvalidColumnName(e.to_string()))?;

        let priority_str: String = row.get("priority")?;
        let priority = match priority_str.as_str() {
            "low" => Priority::Low,
            "high" => Priority::High,
            "critical" => Priority::Critical,
            _ => Priority::Normal,
        };

        let security_subject: String = row.get("security_subject")?;
        let security_scope: String = row.get("security_scope")?;
        let security = SecurityContext {
            subject: match security_subject.as_str() {
                "system" => tiny_mite_domain::Subject::System,
                s if s.starts_with("agent:") => {
                    tiny_mite_domain::Subject::Agent(s.trim_start_matches("agent:").to_owned())
                }
                _ => tiny_mite_domain::Subject::User,
            },
            scope: match security_scope.as_str() {
                "workspace" => tiny_mite_domain::SecurityScope::Workspace,
                "system" => tiny_mite_domain::SecurityScope::System,
                _ => tiny_mite_domain::SecurityScope::Project,
            },
        };

        Ok(EventEnvelope {
            id,
            event_type: row.get("event_type")?,
            version: row.get("version")?,
            timestamp: {
                let ts: String = row.get("timestamp")?;
                DateTime::parse_from_rfc3339(&ts)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now())
            },
            correlation_id,
            causation_id,
            source: row.get("source")?,
            priority,
            security,
            payload,
            payload_type: row.get("payload_type")?,
        })
    }

    // ── Query builders ────────────────────────────────────────

    fn build_query_sql(filter: &QueryFilter) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
        let mut sql = String::from("SELECT * FROM events WHERE 1=1");
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref t) = filter.event_type {
            sql.push_str(" AND event_type LIKE ?1");
            params.push(Box::new(format!("{t}%")));
        }
        if let Some(ref cid) = filter.correlation_id {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND correlation_id = ?{idx}"));
            params.push(Box::new(cid.to_string()));
        }
        if let Some(ref eid) = filter.causation_id {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND causation_id = ?{idx}"));
            params.push(Box::new(eid.to_string()));
        }
        if let Some(ref src) = filter.source {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND source = ?{idx}"));
            params.push(Box::new(src.clone()));
        }
        if let Some(ref from) = filter.from_timestamp {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND timestamp >= ?{idx}"));
            params.push(Box::new(from.to_rfc3339()));
        }
        if let Some(ref to) = filter.to_timestamp {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND timestamp <= ?{idx}"));
            params.push(Box::new(to.to_rfc3339()));
        }

        sql.push_str(" ORDER BY timestamp ASC");

        if let Some(limit) = filter.limit {
            let idx = params.len() + 1;
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params.push(Box::new(limit as i64));
        }
        if let Some(offset) = filter.offset {
            let idx = params.len() + 1;
            sql.push_str(&format!(" OFFSET ?{idx}"));
            params.push(Box::new(offset as i64));
        }

        (sql, params)
    }
}

// ---------------------------------------------------------------------------
// EventStore implementation for SqliteEventStore
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl EventStore for SqliteEventStore {
    async fn append(&self, envelope: &EventEnvelope) -> Result<(), StoreError> {
        let conn = self.conn.lock().await;
        let id_str = envelope.id.to_string();
        let priority_str = envelope.priority.to_string();
        let subject_str = match &envelope.security.subject {
            tiny_mite_domain::Subject::User => "user".to_owned(),
            tiny_mite_domain::Subject::System => "system".to_owned(),
            tiny_mite_domain::Subject::Agent(a) => format!("agent:{a}"),
        };
        let scope_str = match envelope.security.scope {
            tiny_mite_domain::SecurityScope::Project => "project",
            tiny_mite_domain::SecurityScope::Workspace => "workspace",
            tiny_mite_domain::SecurityScope::System => "system",
        };
        let payload_json = serde_json::to_string(&envelope.payload)?;

        // INSERT OR IGNORE ensures idempotency for duplicate event IDs
        conn.execute(
            "INSERT OR IGNORE INTO events
             (id, event_type, version, timestamp, correlation_id, causation_id,
              source, priority, security_subject, security_scope,
              payload_json, payload_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id_str,
                envelope.event_type,
                envelope.version,
                envelope.timestamp.to_rfc3339(),
                envelope.correlation_id.as_ref().map(|c| c.to_string()),
                envelope.causation_id.as_ref().map(|c| c.to_string()),
                envelope.source,
                priority_str,
                subject_str,
                scope_str,
                payload_json,
                envelope.payload_type,
            ],
        )?;

        debug!(event_id = %id_str, event_type = %envelope.event_type, "Event persisted");
        Ok(())
    }

    async fn append_batch(&self, envelopes: &[EventEnvelope]) -> Result<(), StoreError> {
        if envelopes.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().await;
        // Use a transaction for atomic batch insert
        conn.execute("BEGIN IMMEDIATE", [])?;

        let result = (|| -> Result<(), StoreError> {
            for envelope in envelopes {
                let id_str = envelope.id.to_string();
                let priority_str = envelope.priority.to_string();
                let subject_str = match &envelope.security.subject {
                    tiny_mite_domain::Subject::User => "user".to_owned(),
                    tiny_mite_domain::Subject::System => "system".to_owned(),
                    tiny_mite_domain::Subject::Agent(a) => format!("agent:{a}"),
                };
                let scope_str = match envelope.security.scope {
                    tiny_mite_domain::SecurityScope::Project => "project",
                    tiny_mite_domain::SecurityScope::Workspace => "workspace",
                    tiny_mite_domain::SecurityScope::System => "system",
                };
                let payload_json = serde_json::to_string(&envelope.payload)?;

                conn.execute(
                    "INSERT OR IGNORE INTO events
                     (id, event_type, version, timestamp, correlation_id, causation_id,
                      source, priority, security_subject, security_scope,
                      payload_json, payload_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        id_str,
                        envelope.event_type,
                        envelope.version,
                        envelope.timestamp.to_rfc3339(),
                        envelope.correlation_id.as_ref().map(|c| c.to_string()),
                        envelope.causation_id.as_ref().map(|c| c.to_string()),
                        envelope.source,
                        priority_str,
                        subject_str,
                        scope_str,
                        payload_json,
                        envelope.payload_type,
                    ],
                )?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute("COMMIT", [])?;
                debug!(count = envelopes.len(), "Batch events persisted");
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                error!(error = %e, "Batch insert failed — rolled back");
                Err(e)
            }
        }
    }

    async fn get(&self, id: &EventId) -> Result<Option<EventEnvelope>, StoreError> {
        let conn = self.conn.lock().await;
        let id_str = id.to_string();
        let result = conn
            .query_row("SELECT * FROM events WHERE id = ?1", params![id_str], |row| {
                Self::row_to_envelope(row)
            })
            .optional()?;
        Ok(result)
    }

    async fn query(&self, filter: QueryFilter) -> Result<Vec<EventEnvelope>, StoreError> {
        let conn = self.conn.lock().await;
        let (sql, param_refs) = Self::build_query_sql(&filter);

        // Convert Box<dyn ToSql> to &dyn ToSql references
        let param_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_refs.iter().map(|p| p.as_ref() as &dyn rusqlite::types::ToSql).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_slice.as_slice(), |row| Self::row_to_envelope(row))?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    async fn replay(&self, filter: ReplayFilter) -> Result<Vec<EventEnvelope>, StoreError> {
        let conn = self.conn.lock().await;
        let mut sql = String::from("SELECT * FROM events WHERE 1=1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // After checkpoint: replay events after the checkpointed event's timestamp
        if let Some(ref checkpoint) = filter.after_checkpoint {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(
                " AND (timestamp > ?{idx} OR (timestamp = ?{idx} AND id > ?{}))",
                idx + 1
            ));
            param_values.push(Box::new(checkpoint.last_timestamp.to_rfc3339()));
            param_values.push(Box::new(checkpoint.last_event_id.clone()));
        } else if let Some(ref after_id) = filter.after_event_id {
            // After a specific event: get that event's timestamp first
            let after_id_str = after_id.to_string();
            let after_ts: Option<String> = conn
                .query_row(
                    "SELECT timestamp FROM events WHERE id = ?1",
                    params![after_id_str],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(ts) = after_ts {
                let idx = param_values.len() + 1;
                sql.push_str(&format!(
                    " AND (timestamp > ?{idx} OR (timestamp = ?{idx} AND id > ?{}))",
                    idx + 1
                ));
                param_values.push(Box::new(ts));
                param_values.push(Box::new(after_id_str));
            }
        }

        if let Some(ref event_type) = filter.event_type {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND event_type LIKE ?{idx}"));
            param_values.push(Box::new(format!("{event_type}%")));
        }
        if let Some(ref from) = filter.from_timestamp {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND timestamp >= ?{idx}"));
            param_values.push(Box::new(from.to_rfc3339()));
        }
        if let Some(ref to) = filter.to_timestamp {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" AND timestamp <= ?{idx}"));
            param_values.push(Box::new(to.to_rfc3339()));
        }

        sql.push_str(" ORDER BY timestamp ASC, id ASC");

        if let Some(limit) = filter.limit {
            let idx = param_values.len() + 1;
            sql.push_str(&format!(" LIMIT ?{idx}"));
            param_values.push(Box::new(limit as i64));
        }

        let param_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref() as &dyn rusqlite::types::ToSql).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(param_slice.as_slice(), |row| Self::row_to_envelope(row))?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    async fn mark_processed(
        &self,
        consumer_id: &str,
        event_id: &EventId,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().await;
        let event_id_str = event_id.to_string();

        // Read the event timestamp
        let ts: Option<String> = conn
            .query_row("SELECT timestamp FROM events WHERE id = ?1", params![event_id_str], |row| {
                row.get(0)
            })
            .optional()?;

        let timestamp = ts.unwrap_or_else(|| Utc::now().to_rfc3339());
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO checkpoints (consumer_id, last_event_id, last_timestamp, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(consumer_id) DO UPDATE SET
                last_event_id = excluded.last_event_id,
                last_timestamp = excluded.last_timestamp,
                updated_at = excluded.updated_at",
            params![consumer_id, event_id_str, timestamp, now],
        )?;

        debug!(consumer_id, event_id = %event_id_str, "Checkpoint updated");
        Ok(())
    }

    async fn get_checkpoint(&self, consumer_id: &str) -> Result<Option<Checkpoint>, StoreError> {
        let conn = self.conn.lock().await;
        let result = conn
            .query_row(
                "SELECT last_event_id, last_timestamp, updated_at FROM checkpoints WHERE consumer_id = ?1",
                params![consumer_id],
                |row| {
                    Ok(Checkpoint {
                        last_event_id: row.get(0)?,
                        last_timestamp: {
                            let ts: String = row.get(1)?;
                            DateTime::parse_from_rfc3339(&ts)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now())
                        },
                        updated_at: {
                            let ts: String = row.get(2)?;
                            DateTime::parse_from_rfc3339(&ts)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now())
                        },
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    async fn prune(&self, filter: PruneFilter) -> Result<usize, StoreError> {
        let conn = self.conn.lock().await;
        let ts = filter.before_timestamp.to_rfc3339();

        let count = if let Some(ref event_type) = filter.event_type {
            conn.execute(
                "DELETE FROM events WHERE timestamp < ?1 AND event_type LIKE ?2",
                params![ts, format!("{event_type}%")],
            )?
        } else {
            conn.execute("DELETE FROM events WHERE timestamp < ?1", params![ts])?
        };

        info!(pruned = count, "Pruned old events");
        Ok(count)
    }

    async fn health_check(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().await;
        // Verify we can query the migrations table
        let _version: u32 = conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM _migrations", [], |row| row.get(0))
            .map_err(|e| StoreError::Database(e))?;

        // Verify the events table exists
        conn.query_row("SELECT COUNT(*) FROM events", [], |_| Ok(()))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Event;
    use tiny_mite_domain::{SecurityContext, SecurityScope, Subject};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestPayload {
        value: i32,
    }

    impl Event for TestPayload {
        fn event_type(&self) -> &'static str {
            "test.payload"
        }
    }

    fn make_envelope(value: i32) -> EventEnvelope {
        let event = TestPayload { value };
        EventEnvelope::wrap(&event, "test-source", None, None, SecurityContext::default())
            .expect("wrap")
    }

    async fn new_store() -> SqliteEventStore {
        SqliteEventStore::new_in_memory().expect("in-memory store")
    }

    // ── Basic persistence ─────────────────────────────────────

    #[tokio::test]
    async fn append_and_retrieve() {
        let store = new_store().await;
        let env = make_envelope(42);
        store.append(&env).await.expect("append");

        let retrieved = store.get(&env.id).await.expect("get").expect("present");
        assert_eq!(retrieved.payload["value"], 42);
        assert_eq!(retrieved.event_type, "test.payload");
    }

    #[tokio::test]
    async fn get_nonexistent() {
        let store = new_store().await;
        let result = store.get(&EventId::new()).await.expect("get");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn duplicate_append_is_idempotent() {
        let store = new_store().await;
        let env = make_envelope(1);

        store.append(&env).await.expect("first append");
        store.append(&env).await.expect("second append"); // should not error

        // Should only have one event
        let all = store
            .query(QueryFilter { limit: Some(100), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(all.len(), 1);
    }

    // ── Batch ─────────────────────────────────────────────────

    #[tokio::test]
    async fn append_batch_atomic() {
        let store = new_store().await;
        let batch: Vec<_> = (1..=5).map(make_envelope).collect();
        store.append_batch(&batch).await.expect("batch");

        let all = store
            .query(QueryFilter { limit: Some(100), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(all.len(), 5);
    }

    #[tokio::test]
    async fn append_empty_batch() {
        let store = new_store().await;
        store.append_batch(&[]).await.expect("empty batch");
        // No crash, no events
        let all = store
            .query(QueryFilter { limit: Some(100), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(all.len(), 0);
    }

    // ── Query ─────────────────────────────────────────────────

    #[tokio::test]
    async fn query_by_event_type() {
        let store = new_store().await;
        store.append(&make_envelope(1)).await.expect("append");
        store.append(&make_envelope(2)).await.expect("append");

        let results = store
            .query(QueryFilter { event_type: Some("test.payload".into()), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn query_by_correlation() {
        let store = new_store().await;
        let cid = CorrelationId::new();
        let event = TestPayload { value: 7 };
        let env = EventEnvelope::wrap(&event, "test", Some(cid), None, SecurityContext::default())
            .expect("wrap");
        store.append(&env).await.expect("append");

        let results = store
            .query(QueryFilter { correlation_id: Some(cid), ..Default::default() })
            .await
            .expect("query");
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn query_with_pagination() {
        let store = new_store().await;
        let batch: Vec<_> = (0..10).map(make_envelope).collect();
        store.append_batch(&batch).await.expect("batch");

        let page1 = store
            .query(QueryFilter { limit: Some(3), offset: None, ..Default::default() })
            .await
            .expect("page1");
        assert_eq!(page1.len(), 3);

        let page2 = store
            .query(QueryFilter { limit: Some(3), offset: Some(3), ..Default::default() })
            .await
            .expect("page2");
        assert_eq!(page2.len(), 3);

        // Pages should be disjoint
        let ids1: Vec<_> = page1.iter().map(|e| e.id).collect();
        let ids2: Vec<_> = page2.iter().map(|e| e.id).collect();
        for id in ids1 {
            assert!(!ids2.contains(&id));
        }
    }

    // ── Replay ────────────────────────────────────────────────

    #[tokio::test]
    async fn replay_all_events() {
        let store = new_store().await;
        store.append_batch(&(0..5).map(make_envelope).collect::<Vec<_>>()).await.expect("batch");

        let replayed = store
            .replay(ReplayFilter {
                after_checkpoint: None,
                after_event_id: None,
                event_type: None,
                from_timestamp: None,
                to_timestamp: None,
                limit: None,
            })
            .await
            .expect("replay");
        assert_eq!(replayed.len(), 5);
    }

    #[tokio::test]
    async fn replay_after_checkpoint() {
        let store = new_store().await;
        let env1 = make_envelope(1);
        let env2 = make_envelope(2);
        let env3 = make_envelope(3);

        store.append_batch(&[env1.clone(), env2.clone(), env3.clone()]).await.expect("batch");

        // Mark env1 as processed
        store.mark_processed("consumer-1", &env1.id).await.expect("checkpoint");

        // Replay after checkpoint
        let cp = store.get_checkpoint("consumer-1").await.expect("get_cp").expect("cp");
        let replayed = store
            .replay(ReplayFilter { after_checkpoint: Some(cp), ..Default::default() })
            .await
            .expect("replay");

        // Should get env2 and env3, not env1
        assert_eq!(replayed.len(), 2);
        let ids: Vec<_> = replayed.iter().map(|e| e.id).collect();
        assert!(!ids.contains(&env1.id));
        assert!(ids.contains(&env2.id));
        assert!(ids.contains(&env3.id));
    }

    #[tokio::test]
    async fn replay_filtered_by_type() {
        let store = new_store().await;

        // Create an event with a different type
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        struct OtherEvent {
            x: i32,
        }
        impl Event for OtherEvent {
            fn event_type(&self) -> &'static str {
                "other.type"
            }
        }
        let other = EventEnvelope::wrap(
            &OtherEvent { x: 1 },
            "test",
            None,
            None,
            SecurityContext::default(),
        )
        .expect("wrap");

        let normal = make_envelope(42);
        store.append_batch(&[normal.clone(), other]).await.expect("batch");

        let replayed = store
            .replay(ReplayFilter { event_type: Some("test.payload".into()), ..Default::default() })
            .await
            .expect("replay");
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].payload["value"], 42);
    }

    #[tokio::test]
    async fn replay_empty() {
        let store = new_store().await;
        let replayed = store.replay(ReplayFilter::default()).await.expect("replay");
        assert!(replayed.is_empty());
    }

    // ── Checkpoints ───────────────────────────────────────────

    #[tokio::test]
    async fn create_and_update_checkpoint() {
        let store = new_store().await;
        let env = make_envelope(100);
        store.append(&env).await.expect("append");

        store.mark_processed("consumer-A", &env.id).await.expect("mark");

        let cp = store.get_checkpoint("consumer-A").await.expect("get").expect("cp");
        assert_eq!(cp.last_event_id, env.id.to_string());

        // Update with a newer event
        let env2 = make_envelope(200);
        store.append(&env2).await.expect("append");
        store.mark_processed("consumer-A", &env2.id).await.expect("mark2");

        let cp2 = store.get_checkpoint("consumer-A").await.expect("get").expect("cp2");
        assert_eq!(cp2.last_event_id, env2.id.to_string());
    }

    #[tokio::test]
    async fn independent_consumer_checkpoints() {
        let store = new_store().await;
        let env = make_envelope(1);
        store.append(&env).await.expect("append");

        store.mark_processed("consumer-1", &env.id).await.expect("cp1");
        store.mark_processed("consumer-2", &env.id).await.expect("cp2");

        let cp1 = store.get_checkpoint("consumer-1").await.expect("get1");
        let cp2 = store.get_checkpoint("consumer-2").await.expect("get2");

        assert!(cp1.is_some());
        assert!(cp2.is_some());
    }

    #[tokio::test]
    async fn checkpoint_for_nonexistent_consumer() {
        let store = new_store().await;
        let cp = store.get_checkpoint("no-such-consumer").await.expect("get");
        assert!(cp.is_none());
    }

    // ── Crash / recovery ──────────────────────────────────────

    #[tokio::test]
    async fn events_survive_reopen() {
        // Use a temp file so we can reopen
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test.db");

        let env = make_envelope(999);
        {
            let store = SqliteEventStore::open(&db_path).await.expect("open");
            store.append(&env).await.expect("append");
        } // store dropped — connection closed

        {
            let store = SqliteEventStore::open(&db_path).await.expect("reopen");
            let retrieved = store.get(&env.id).await.expect("get").expect("present");
            assert_eq!(retrieved.payload["value"], 999);
        }
    }

    #[tokio::test]
    async fn unprocessed_events_replayable_after_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test2.db");

        let env1 = make_envelope(1);
        let env2 = make_envelope(2);
        {
            let store = SqliteEventStore::open(&db_path).await.expect("open");
            store.append_batch(&[env1.clone(), env2.clone()]).await.expect("batch");
            store.mark_processed("consumer-x", &env1.id).await.expect("cp1");
            // env2 is NOT marked as processed
        }

        {
            let store = SqliteEventStore::open(&db_path).await.expect("reopen");
            let cp = store.get_checkpoint("consumer-x").await.expect("get").expect("cp");
            let replayed = store
                .replay(ReplayFilter { after_checkpoint: Some(cp), ..Default::default() })
                .await
                .expect("replay");

            // Only env2 should be replayed
            assert_eq!(replayed.len(), 1);
            assert_eq!(replayed[0].id, env2.id);
        }
    }

    // ── Health check ──────────────────────────────────────────

    #[tokio::test]
    async fn health_check_passes() {
        let store = new_store().await;
        store.health_check().await.expect("healthy");
    }

    // ── Prune ─────────────────────────────────────────────────

    #[tokio::test]
    async fn prune_old_events() {
        let store = new_store().await;
        let env = make_envelope(1);
        store.append(&env).await.expect("append");

        // Prune events older than "now" (which is in the future relative to insertion)
        // Actually, let's prune with a far-future timestamp to ensure deletion
        let count = store
            .prune(PruneFilter {
                before_timestamp: Utc::now()
                    .checked_add_signed(chrono::Duration::hours(1))
                    .unwrap_or(Utc::now()),
                event_type: None,
            })
            .await
            .expect("prune");
        assert_eq!(count, 1);

        let remaining = store.get(&env.id).await.expect("get");
        assert!(remaining.is_none());
    }

    // ── Security ───────────────────────────────────────────────

    #[tokio::test]
    async fn stored_payload_preserves_structure() {
        // Verify that event payloads roundtrip correctly through SQLite
        let store = new_store().await;
        let env = make_envelope(42);
        store.append(&env).await.expect("append");

        let retrieved = store.get(&env.id).await.expect("get").expect("present");
        // The payload should be the same JSON value
        assert_eq!(retrieved.payload, env.payload);
        assert_eq!(retrieved.payload_type, env.payload_type);
    }
}

impl Default for ReplayFilter {
    fn default() -> Self {
        Self {
            after_checkpoint: None,
            after_event_id: None,
            event_type: None,
            from_timestamp: None,
            to_timestamp: None,
            limit: None,
        }
    }
}
