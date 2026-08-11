//! Context compaction — intelligent context reduction for bounded model windows.
//!
//! When context exceeds the model's budget, compaction reduces content
//! while preserving the most important information.

use crate::context::{ContextItem, ContextItemType};

/// Strategy for compacting context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStrategy {
    /// Remove oldest items first (FIFO).
    DropOldest,
    /// Remove lowest priority items first.
    DropLowPriority,
    /// Truncate content while keeping headers.
    TruncateToHeadline,
    /// Summarize and merge adjacent items of same type.
    MergeAdjacent,
}

/// Result of a context compaction pass.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// Number of items removed.
    pub items_removed: usize,
    /// Number of items truncated.
    pub items_truncated: usize,
    /// Estimated tokens before compaction.
    pub tokens_before: usize,
    /// Estimated tokens after compaction.
    pub tokens_after: usize,
    /// Whether compaction achieved the target budget.
    pub target_met: bool,
}

/// Compacts context items to fit within a token budget.
pub struct ContextCompactor {
    /// Maximum token budget after compaction.
    budget: usize,
    /// Strategy to use.
    strategy: CompactionStrategy,
    /// Maximum content length after truncation.
    max_content_length: usize,
}

impl ContextCompactor {
    /// Create a new compactor.
    #[must_use]
    pub fn new(budget: usize, strategy: CompactionStrategy) -> Self {
        Self { budget, strategy, max_content_length: 200 }
    }

    /// Compact a list of context items.
    #[must_use]
    pub fn compact(&self, items: Vec<ContextItem>) -> (Vec<ContextItem>, CompactionResult) {
        let tokens_before: usize = items.iter().map(|i| i.token_count).sum();
        let mut result = CompactionResult {
            items_removed: 0,
            items_truncated: 0,
            tokens_before,
            tokens_after: 0,
            target_met: false,
        };

        if tokens_before <= self.budget {
            result.tokens_after = tokens_before;
            result.target_met = true;
            return (items, result);
        }

        let mut items = items;

        match self.strategy {
            CompactionStrategy::DropOldest => {
                let mut total = tokens_before;
                while total > self.budget && !items.is_empty() {
                    total -= items[0].token_count;
                    items.remove(0);
                    result.items_removed += 1;
                }
            }
            CompactionStrategy::DropLowPriority => {
                items.sort_by_key(|i| i.priority);
                let mut total = tokens_before;
                while total > self.budget && !items.is_empty() {
                    total -= items[0].token_count;
                    items.remove(0);
                    result.items_removed += 1;
                }
            }
            CompactionStrategy::TruncateToHeadline => {
                let mut total = tokens_before;
                for item in &mut items {
                    if total <= self.budget {
                        break;
                    }
                    if item.content.len() > self.max_content_length {
                        let old_len = item.content.len();
                        item.content.truncate(self.max_content_length);
                        item.content.push_str("...");
                        let new_tokens = item.content.len() / 3;
                        total = total.saturating_sub(item.token_count.saturating_sub(new_tokens));
                        item.token_count = new_tokens;
                        result.items_truncated += 1;
                    }
                }
            }
            CompactionStrategy::MergeAdjacent => {
                // Merge adjacent items of the same type to reduce overhead
                let mut merged = Vec::new();
                let mut i = 0;
                while i < items.len() {
                    if i + 1 < items.len() && items[i].item_type == items[i + 1].item_type {
                        let mut combined = items[i].content.clone();
                        combined.push_str(" | ");
                        combined.push_str(&items[i + 1].content);
                        let mut item = items[i].clone();
                        item.content = combined;
                        item.token_count = item.content.len() / 3;
                        merged.push(item);
                        i += 2;
                        result.items_removed += 1;
                    } else {
                        merged.push(items[i].clone());
                        i += 1;
                    }
                }
                items = merged;
            }
        }

        result.tokens_after = items.iter().map(|i| i.token_count).sum();
        result.target_met = result.tokens_after <= self.budget;
        (items, result)
    }

    /// Set the maximum content length for truncation.
    #[must_use]
    pub fn with_max_content_length(mut self, len: usize) -> Self {
        self.max_content_length = len;
        self
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Authority;
    use chrono::Utc;

    fn make_item(id: &str, priority: u32, tokens: usize) -> ContextItem {
        ContextItem {
            id: id.into(),
            item_type: ContextItemType::UserMessage,
            content: "x".repeat(tokens * 3),
            token_count: tokens,
            priority,
            relevance: 50,
            authority: Authority::User,
            pinned: false,
            sensitivity: crate::context::Sensitivity::Public,
            timestamp: Utc::now(),
            source_id: None,
            correlation_id: None,
            task_id: None,
            memory_id: None,
            document_id: None,
        }
    }

    #[test]
    fn no_compaction_when_under_budget() {
        let items = vec![make_item("a", 1, 100), make_item("b", 2, 200)];
        let compactor = ContextCompactor::new(500, CompactionStrategy::DropOldest);
        let (result, stats) = compactor.compact(items);
        assert_eq!(result.len(), 2);
        assert!(stats.target_met);
    }

    #[test]
    fn drop_oldest_removes_head() {
        let items = vec![make_item("a", 1, 300), make_item("b", 2, 300), make_item("c", 3, 300)];
        let compactor = ContextCompactor::new(600, CompactionStrategy::DropOldest);
        let (result, stats) = compactor.compact(items);
        assert_eq!(result.len(), 2);
        assert!(stats.items_removed >= 1);
        assert!(stats.target_met);
    }

    #[test]
    fn truncate_reduces_content() {
        let items = vec![make_item("a", 1, 500), make_item("b", 2, 100)];
        let compactor = ContextCompactor::new(300, CompactionStrategy::TruncateToHeadline)
            .with_max_content_length(50);
        let (result, stats) = compactor.compact(items);
        assert!(stats.items_truncated >= 1);
        assert!(result[0].content.len() <= 53); // 50 + "..."
    }

    #[test]
    fn merge_adjacent_combines_same_type() {
        let mut a = make_item("a", 1, 50);
        let mut b = make_item("b", 2, 50);
        a.item_type = ContextItemType::ToolResult;
        b.item_type = ContextItemType::ToolResult;
        let items = vec![a, b, make_item("c", 3, 50)];
        let compactor = ContextCompactor::new(1000, CompactionStrategy::MergeAdjacent);
        let (result, stats) = compactor.compact(items);
        assert_eq!(result.len(), 2); // a+b merged, c separate
        assert_eq!(stats.items_removed, 1);
    }
}
