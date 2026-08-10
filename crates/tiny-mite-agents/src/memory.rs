//! Working memory — task-scoped, bounded, importance-ranked storage.
//!
//! Working memory exists only for the duration of an active task.
//! It supports snapshot/restore for interruption recovery and
//! integrates with the ContextEngine for prompt assembly.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_mite_domain::{CorrelationId, TaskId};

use crate::planner::Plan;

// ── Memory item category ────────────────────────────────────────

/// Category of working memory content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryCategory {
    /// Task objective.
    Objective,
    /// Task constraint.
    Constraint,
    /// An observation or discovered fact.
    Observation,
    /// A stated fact (may be uncertain).
    Fact,
    /// An assumption being held.
    Assumption,
    /// Output from a tool execution.
    ToolResult,
    /// Current plan state.
    PlanState,
    /// Verification result of a step.
    Verification,
    /// An unresolved question.
    Question,
    /// An error encountered.
    Error,
    /// Other/uncategorized.
    Other,
}

// ── Memory item ─────────────────────────────────────────────────

/// A single entry in working memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingMemoryItem {
    /// Unique identifier.
    pub id: String,
    /// Content category.
    pub category: MemoryCategory,
    /// The content string.
    pub content: String,
    /// Importance score (0–100, higher = more important).
    pub importance: u32,
    /// When the item was created.
    pub created_at: DateTime<Utc>,
    /// Optional expiration time.
    pub expires_at: Option<DateTime<Utc>>,
    /// Source of this memory (e.g., tool name, plan step ID).
    pub source: Option<String>,
    /// Correlation ID for tracing.
    pub correlation_id: Option<CorrelationId>,
    /// Whether this item must never be evicted.
    pub mandatory: bool,
    /// Estimated token count.
    pub token_count: usize,
    /// Access count (for recency/frequency scoring).
    pub access_count: u32,
}

impl WorkingMemoryItem {
    /// Create a new memory item.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        category: MemoryCategory,
        content: impl Into<String>,
    ) -> Self {
        let content: String = content.into();
        let token_count = content.len() / 3;
        Self {
            id: id.into(),
            category,
            content,
            importance: 10,
            created_at: Utc::now(),
            expires_at: None,
            source: None,
            correlation_id: None,
            mandatory: false,
            token_count,
            access_count: 0,
        }
    }

    /// Mark as mandatory (never evicted).
    #[must_use]
    pub fn mandatory(mut self) -> Self {
        self.mandatory = true;
        self
    }

    /// Set importance.
    #[must_use]
    pub fn with_importance(mut self, imp: u32) -> Self {
        self.importance = imp.min(100);
        self
    }

    /// Set source.
    #[must_use]
    pub fn from_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set expiration.
    #[must_use]
    pub fn expires_in(mut self, duration: Duration) -> Self {
        self.expires_at = Some(Utc::now() + duration);
        self
    }

    /// Returns `true` if this item has expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at.map_or(false, |t| Utc::now() >= t)
    }
}

// ── Working memory snapshot ──────────────────────────────────────

/// A serializable snapshot of working memory state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingMemorySnapshot {
    /// All items at snapshot time.
    pub items: Vec<WorkingMemoryItem>,
    /// When the snapshot was taken.
    pub captured_at: DateTime<Utc>,
    /// Task ID associated with this memory.
    pub task_id: Option<TaskId>,
    /// Total items in memory.
    pub item_count: usize,
    /// Total estimated tokens.
    pub total_tokens: usize,
}

// ── Working memory ──────────────────────────────────────────────

/// Task-scoped working memory with importance-based eviction.
///
/// # Bounds
///
/// - Maximum number of items (default: 100)
/// - Maximum estimated tokens (default: 32,768)
/// - Automatic expiration of timed items
///
/// # Eviction
///
/// When bounds are exceeded, eviction removes the lowest-scored
/// non-mandatory items first. Score = importance * 100 + recency_bonus.
impl std::fmt::Debug for WorkingMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkingMemory")
            .field("items", &self.items.len())
            .field("max_items", &self.max_items)
            .field("total_tokens", &self.total_tokens())
            .finish()
    }
}

impl Clone for WorkingMemory {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            max_items: self.max_items,
            max_tokens: self.max_tokens,
            task_id: self.task_id,
        }
    }
}

pub struct WorkingMemory {
    /// All memory items, keyed by ID.
    items: HashMap<String, WorkingMemoryItem>,
    /// Maximum items before eviction.
    max_items: usize,
    /// Maximum estimated tokens before eviction.
    max_tokens: usize,
    /// Task ID this memory belongs to.
    task_id: Option<TaskId>,
}

impl WorkingMemory {
    /// Create a new working memory instance.
    #[must_use]
    pub fn new() -> Self {
        Self { items: HashMap::new(), max_items: 100, max_tokens: 32_768, task_id: None }
    }

    /// Set the maximum number of items.
    #[must_use]
    pub fn with_max_items(mut self, n: usize) -> Self {
        self.max_items = n;
        self
    }

    /// Set the maximum estimated tokens.
    #[must_use]
    pub fn with_max_tokens(mut self, n: usize) -> Self {
        self.max_tokens = n;
        self
    }

    /// Associate with a task.
    pub fn set_task(&mut self, task_id: TaskId) {
        self.task_id = Some(task_id);
    }

    /// Insert or update a memory item. Triggers eviction if bounds exceeded.
    pub fn insert(&mut self, item: WorkingMemoryItem) {
        self.items.insert(item.id.clone(), item);
        self.evict_if_needed();
    }

    /// Get a reference to an item.
    pub fn get(&mut self, id: &str) -> Option<&mut WorkingMemoryItem> {
        let item = self.items.get_mut(id)?;
        item.access_count += 1;
        Some(item)
    }

    /// Remove an item by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        self.items.remove(id).is_some()
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Number of items in memory.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if memory is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Total estimated tokens across all items.
    #[must_use]
    pub fn total_tokens(&self) -> usize {
        self.items.values().map(|i| i.token_count).sum()
    }

    /// List all items, ordered by importance descending.
    #[must_use]
    pub fn items_by_importance(&self) -> Vec<&WorkingMemoryItem> {
        let mut items: Vec<&WorkingMemoryItem> = self.items.values().collect();
        items.sort_by_key(|i| -(i.importance as i64));
        items
    }

    /// Remove expired items. Returns count removed.
    pub fn purge_expired(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|_, item| !item.is_expired());
        before - self.items.len()
    }

    /// Create a snapshot.
    #[must_use]
    pub fn snapshot(&self) -> WorkingMemorySnapshot {
        let items: Vec<_> = self.items.values().cloned().collect();
        WorkingMemorySnapshot {
            total_tokens: items.iter().map(|i| i.token_count).sum(),
            item_count: items.len(),
            items,
            captured_at: Utc::now(),
            task_id: self.task_id,
        }
    }

    /// Restore from a snapshot.
    pub fn restore(&mut self, snapshot: &WorkingMemorySnapshot) {
        self.items.clear();
        for item in &snapshot.items {
            self.items.insert(item.id.clone(), item.clone());
        }
        self.task_id = snapshot.task_id;
    }

    // ── Private ───────────────────────────────────────────────

    fn evict_if_needed(&mut self) {
        while self.items.len() > self.max_items {
            if let Some(id) = self.find_eviction_candidate() {
                self.items.remove(&id);
            } else {
                break;
            }
        }
        while self.total_tokens() > self.max_tokens && !self.items.is_empty() {
            if let Some(id) = self.find_eviction_candidate() {
                self.items.remove(&id);
            } else {
                break;
            }
        }
    }

    fn find_eviction_candidate(&self) -> Option<String> {
        self.items
            .values()
            .filter(|i| !i.mandatory && !i.is_expired())
            .min_by_key(|i| self.eviction_score(i))
            .or_else(|| {
                self.items
                    .values()
                    .filter(|i| i.is_expired())
                    .min_by_key(|i| self.eviction_score(i))
            })
            .map(|i| i.id.clone())
    }

    fn eviction_score(&self, item: &WorkingMemoryItem) -> i64 {
        // Lower = evict first. Mandatory items have a huge bonus and won't be evicted.
        let importance = item.importance as i64 * 100;
        let age_ms = (Utc::now() - item.created_at).num_milliseconds().max(0);
        let recency_penalty = -(age_ms / 1000).min(3600); // up to -3600 for >1hr old
        let freq_bonus = (item.access_count as i64).min(100) * 5;
        importance + recency_penalty + freq_bonus
    }

    /// Load plan steps into working memory.
    pub fn load_plan(&mut self, plan: &Plan) {
        self.insert(
            WorkingMemoryItem::new(
                "plan:objective",
                MemoryCategory::Objective,
                &plan.task_description,
            )
            .mandatory()
            .with_importance(100),
        );
        for step in &plan.steps {
            self.insert(
                WorkingMemoryItem::new(
                    format!("plan:step:{}", step.id),
                    MemoryCategory::PlanState,
                    format!(
                        "Step '{}': {} ({})",
                        step.id,
                        step.description,
                        if step.dependencies.is_empty() { "ready" } else { "pending" }
                    ),
                )
                .with_importance(70)
                .from_source(format!("plan_step:{}", step.id)),
            );
        }
    }
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut mem = WorkingMemory::new();
        mem.insert(WorkingMemoryItem::new("a", MemoryCategory::Fact, "hello"));
        let item = mem.get("a").expect("present");
        assert_eq!(item.content, "hello");
        assert_eq!(item.access_count, 1);
    }

    #[test]
    fn eviction_by_count() {
        let mut mem = WorkingMemory::new().with_max_items(2);
        mem.insert(WorkingMemoryItem::new("a", MemoryCategory::Fact, "a").with_importance(10));
        mem.insert(WorkingMemoryItem::new("b", MemoryCategory::Fact, "b").with_importance(90));
        mem.insert(WorkingMemoryItem::new("c", MemoryCategory::Fact, "c").with_importance(50));
        assert_eq!(mem.len(), 2);
        assert!(mem.get("b").is_some());
        assert!(mem.get("c").is_some());
    }

    #[test]
    fn mandatory_items_survive() {
        let mut mem = WorkingMemory::new().with_max_items(2);
        mem.insert(
            WorkingMemoryItem::new("a", MemoryCategory::Constraint, "must")
                .mandatory()
                .with_importance(5),
        );
        mem.insert(WorkingMemoryItem::new("b", MemoryCategory::Fact, "b").with_importance(90));
        mem.insert(WorkingMemoryItem::new("c", MemoryCategory::Fact, "c").with_importance(50));
        assert!(mem.get("a").is_some());
    }

    #[test]
    fn eviction_by_tokens() {
        let mut mem = WorkingMemory::new().with_max_tokens(30);
        mem.insert(WorkingMemoryItem::new("big", MemoryCategory::Fact, &"x".repeat(200)));
        mem.insert(WorkingMemoryItem::new("small", MemoryCategory::Fact, "hi").mandatory());
        assert!(mem.get("small").is_some());
    }

    #[test]
    fn snapshot_and_restore() {
        let mut mem = WorkingMemory::new();
        mem.insert(WorkingMemoryItem::new("a", MemoryCategory::Fact, "data"));
        let snap = mem.snapshot();
        mem.clear();
        assert!(mem.is_empty());
        mem.restore(&snap);
        assert_eq!(mem.get("a").unwrap().content, "data");
    }

    #[test]
    fn items_by_importance() {
        let mut mem = WorkingMemory::new();
        mem.insert(WorkingMemoryItem::new("low", MemoryCategory::Fact, "").with_importance(10));
        mem.insert(
            WorkingMemoryItem::new("high", MemoryCategory::Constraint, "").with_importance(90),
        );
        let ordered = mem.items_by_importance();
        assert_eq!(ordered[0].id, "high");
    }

    #[test]
    fn expiration() {
        let mut mem = WorkingMemory::new().with_max_items(5);
        mem.insert(
            WorkingMemoryItem::new("exp", MemoryCategory::Fact, "old")
                .expires_in(Duration::milliseconds(-1)),
        );
        mem.insert(WorkingMemoryItem::new("fresh", MemoryCategory::Fact, "new").with_importance(1));
        let purged = mem.purge_expired();
        assert_eq!(purged, 1);
        assert!(mem.get("fresh").is_some());
    }

    #[test]
    fn load_plan() {
        let plan = Plan::new("p1", "implement BST");
        let mut mem = WorkingMemory::new();
        mem.load_plan(&plan);
        assert!(mem.get("plan:objective").is_some());
    }
}
