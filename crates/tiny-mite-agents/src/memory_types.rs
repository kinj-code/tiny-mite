//! Memory architecture — episodic, semantic, procedural, and project memory.
//!
//! Defines memory type abstractions and consolidation interfaces.

use serde::{Deserialize, Serialize};
use tiny_mite_domain::{CorrelationId, MemoryId, TaskId};

// ── Episodic memory ──────────────────────────────────────────────

/// A record of an experience or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicMemory {
    pub id: MemoryId,
    pub task_id: TaskId,
    pub description: String,
    pub outcome: String,
    pub lessons: Vec<String>,
    pub correlation_id: CorrelationId,
}

// ── Semantic memory ──────────────────────────────────────────────

/// A factual piece of knowledge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: MemoryId,
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub source: String,
}

// ── Procedural memory ────────────────────────────────────────────

/// A learned procedure or workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralMemory {
    pub id: MemoryId,
    pub name: String,
    pub steps: Vec<String>,
    pub success_rate: f32,
    pub last_used: chrono::DateTime<chrono::Utc>,
}

// ── Project memory ───────────────────────────────────────────────

/// Project-scoped persistent context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub id: MemoryId,
    pub key: String,
    pub value: String,
    pub scope: String,
}

// ── Memory consolidation ─────────────────────────────────────────

/// Criteria for consolidating working memory into long-term memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationCriteria {
    pub min_importance: u32,
    pub min_access_count: u32,
    pub max_age_days: i64,
}

impl Default for ConsolidationCriteria {
    fn default() -> Self {
        Self { min_importance: 50, min_access_count: 3, max_age_days: 30 }
    }
}

/// Result of a memory consolidation pass.
#[derive(Debug, Clone)]
pub struct ConsolidationResult {
    pub promoted: usize,
    pub archived: usize,
    pub errors: Vec<String>,
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodic_memory_creation() {
        let mem = EpisodicMemory {
            id: MemoryId::new(),
            task_id: TaskId::new(),
            description: "fixed bug".into(),
            outcome: "success".into(),
            lessons: vec!["check nulls".into()],
            correlation_id: CorrelationId::new(),
        };
        assert_eq!(mem.outcome, "success");
    }

    #[test]
    fn consolidation_criteria_defaults() {
        let criteria = ConsolidationCriteria::default();
        assert_eq!(criteria.min_importance, 50);
        assert!(criteria.max_age_days > 0);
    }
}
