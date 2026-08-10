//! Security audit log — records every privileged operation.
//!
//! Every tool execution, capability check, and policy decision produces
//! an immutable audit entry with correlation IDs for traceability.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tiny_mite_domain::CorrelationId;

/// Severity level for audit entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuditLevel {
    Info,
    Warning,
    Error,
    Critical,
}

/// A single audit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique entry identifier.
    pub id: String,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Severity level.
    pub level: AuditLevel,
    /// What operation was performed.
    pub operation: String,
    /// Who/what performed it.
    pub subject: String,
    /// Correlation ID for tracing.
    pub correlation_id: Option<CorrelationId>,
    /// Whether the operation was allowed.
    pub allowed: bool,
    /// Human-readable description.
    pub description: String,
    /// Optional structured data (e.g. tool parameters — redacted).
    pub details: Option<String>,
}

/// A simple in-memory audit log with bounded capacity.
///
/// In production this would be persisted via the EventStore.
#[derive(Debug)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    max_entries: usize,
}

impl AuditLog {
    /// Create a new audit log with the given maximum capacity.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self { entries: Vec::with_capacity(max_entries), max_entries }
    }

    /// Record an audit entry. Oldest entries are dropped when full.
    pub fn record(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Return all entries, most recent last.
    #[must_use]
    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// Number of entries currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str) -> AuditEntry {
        AuditEntry {
            id: id.into(),
            timestamp: Utc::now(),
            level: AuditLevel::Info,
            operation: "test".into(),
            subject: "unit-test".into(),
            correlation_id: None,
            allowed: true,
            description: "test entry".into(),
            details: None,
        }
    }

    #[test]
    fn records_entries() {
        let mut log = AuditLog::new(100);
        log.record(make_entry("1"));
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn bounded_capacity_drops_oldest() {
        let mut log = AuditLog::new(2);
        log.record(make_entry("a"));
        log.record(make_entry("b"));
        log.record(make_entry("c"));
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[0].id, "b");
    }

    #[test]
    fn clear_removes_all() {
        let mut log = AuditLog::new(10);
        log.record(make_entry("1"));
        log.record(make_entry("2"));
        log.clear();
        assert!(log.is_empty());
    }
}
