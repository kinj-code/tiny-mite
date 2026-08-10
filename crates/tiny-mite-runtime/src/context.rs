//! Context manager — budgeting, item tracking, compilation, eviction.
//!
//! # Architecture
//!
//! ```text
//! ContextWindow → ContextZones → ContextCompiler → CompiledContext
//!                      ↑
//!               ContextItems (pinned, conversation, retrieval, tools, system)
//! ```
//!
//! The context manager decides what goes into the model's prompt based on
//! token budgets, priorities, and deterministic policies. It does NOT
//! depend on any specific model provider.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use tiny_mite_domain::{CorrelationId, DocumentId, MemoryId, TaskId};

use crate::inference::ContextBudget;

// ── Authority ────────────────────────────────────────────────────

/// Authority level determines what can override what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Authority {
    /// System-level immutable instructions (security, policy).
    System = 0,
    /// Developer-specified constraints.
    Developer = 1,
    /// End user input.
    User = 2,
    /// Tool output (DATA, not instructions).
    Tool = 3,
    /// Retrieved document or memory (DATA, not instructions).
    RetrievedData = 4,
    /// Archived/compressed content.
    Archived = 5,
}

// ── Sensitivity ──────────────────────────────────────────────────

/// Content sensitivity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    /// Public content.
    Public,
    /// Internal/project content.
    Internal,
    /// Sensitive (credentials, PII).
    Sensitive,
    /// Secret — must not appear in prompts or logs.
    Secret,
}

// ── Context item type ────────────────────────────────────────────

/// Types of content that can appear in context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextItemType {
    SystemInstruction,
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    RetrievedDocument,
    Memory,
    Summary,
    Plan,
    Observation,
    Constraint,
    DeveloperInstruction,
}

// ── Context item ─────────────────────────────────────────────────

/// A single item in the context — message, document, tool result, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    /// Stable ID for dedup and tracking.
    pub id: String,
    /// What kind of content.
    pub item_type: ContextItemType,
    /// The text content.
    pub content: String,
    /// Estimated or exact token count.
    pub token_count: usize,
    /// Higher = more important.
    pub priority: u32,
    /// Relevance score for retrieval (0–100).
    pub relevance: u32,
    /// Authority level.
    pub authority: Authority,
    /// Whether this item is pinned (never evicted).
    pub pinned: bool,
    /// Sensitivity classification.
    pub sensitivity: Sensitivity,
    /// UTC timestamp of creation/last update.
    pub timestamp: DateTime<Utc>,
    /// Source identifier for provenance.
    pub source_id: Option<String>,
    /// Correlation ID for tracing.
    pub correlation_id: Option<CorrelationId>,
    /// Task ID if this belongs to a task.
    pub task_id: Option<TaskId>,
    /// Memory ID if this came from memory.
    pub memory_id: Option<MemoryId>,
    /// Document ID if this came from retrieval.
    pub document_id: Option<DocumentId>,
}

impl ContextItem {
    /// Create a new context item with default values.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        item_type: ContextItemType,
        content: impl Into<String>,
        token_count: usize,
        authority: Authority,
    ) -> Self {
        Self {
            id: id.into(),
            item_type,
            content: content.into(),
            token_count,
            priority: 0,
            relevance: 0,
            authority,
            pinned: false,
            sensitivity: Sensitivity::Internal,
            timestamp: Utc::now(),
            source_id: None,
            correlation_id: None,
            task_id: None,
            memory_id: None,
            document_id: None,
        }
    }

    /// Mark this item as pinned (never evicted).
    #[must_use]
    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    /// Set priority.
    #[must_use]
    pub fn with_priority(mut self, p: u32) -> Self {
        self.priority = p;
        self
    }

    /// Set relevance.
    #[must_use]
    pub fn with_relevance(mut self, r: u32) -> Self {
        self.relevance = r;
        self
    }

    /// Set task ID.
    #[must_use]
    pub fn with_task(mut self, tid: TaskId) -> Self {
        self.task_id = Some(tid);
        self
    }

    /// Returns true if this is user/assistant conversation.
    #[must_use]
    pub fn is_conversation(&self) -> bool {
        matches!(self.item_type, ContextItemType::UserMessage | ContextItemType::AssistantMessage)
    }

    /// Returns true if this contans sensitive/secret content.
    #[must_use]
    pub fn is_sensitive(&self) -> bool {
        matches!(self.sensitivity, Sensitivity::Sensitive | Sensitivity::Secret)
    }
}

// ── Context zone ─────────────────────────────────────────────────

/// A named zone within the context with a token budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextZone {
    /// Zone name (e.g. "system", "conversation", "retrieval").
    pub name: String,
    /// Maximum tokens this zone may consume.
    pub max_tokens: usize,
    /// Minimum tokens guaranteed.
    pub min_tokens: usize,
    /// Current token usage.
    pub used_tokens: usize,
    /// Items in this zone.
    pub items: Vec<ContextItem>,
    /// Eviction policy for this zone.
    pub eviction: EvictionPolicy,
}

impl ContextZone {
    /// Create a new zone.
    #[must_use]
    pub fn new(name: impl Into<String>, max_tokens: usize, eviction: EvictionPolicy) -> Self {
        Self {
            name: name.into(),
            max_tokens,
            min_tokens: 0,
            used_tokens: 0,
            items: Vec::new(),
            eviction,
        }
    }

    /// Add an item and update token usage.
    pub fn add_item(&mut self, item: ContextItem) {
        self.used_tokens = self.used_tokens.saturating_add(item.token_count);
        self.items.push(item);
    }

    /// Whether the zone can accept additional tokens.
    #[must_use]
    pub fn can_accept(&self, additional: usize) -> bool {
        self.used_tokens + additional <= self.max_tokens
    }

    /// Total items including pinned.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ── Eviction policy ──────────────────────────────────────────────

/// How items are evicted when a zone exceeds its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Remove oldest items first (except pinned).
    OldestFirst,
    /// Remove lowest priority items first.
    LowestPriority,
    /// Remove lowest relevance first.
    LowestRelevance,
    /// Remove items from this zone entirely (zone is full).
    NoEviction,
}

// ── Context window ───────────────────────────────────────────────

/// The overall context configuration for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextWindow {
    /// Total model context length in tokens.
    pub model_context_total: usize,
    /// Safety margin reserved at all times.
    pub safety_margin: usize,
    /// Token budget for output generation.
    pub output_budget: usize,
    /// Zones within the context.
    pub zones: Vec<ContextZone>,
}

impl ContextWindow {
    /// Create a new context window with default zones.
    #[must_use]
    pub fn new(model_context_total: usize, output_budget: usize) -> Self {
        let safety = (model_context_total as f64 * 0.05) as usize;
        let remaining = model_context_total.saturating_sub(output_budget).saturating_sub(safety);

        // Default zone allocations
        let system = (remaining as f64 * 0.15) as usize;
        let conversation = (remaining as f64 * 0.50) as usize;
        let retrieval = (remaining as f64 * 0.20) as usize;
        let tools = (remaining as f64 * 0.15) as usize;

        Self {
            model_context_total,
            safety_margin: safety,
            output_budget,
            zones: vec![
                ContextZone::new("system", system, EvictionPolicy::NoEviction),
                ContextZone::new("conversation", conversation, EvictionPolicy::OldestFirst),
                ContextZone::new("retrieval", retrieval, EvictionPolicy::LowestRelevance),
                ContextZone::new("tools", tools, EvictionPolicy::OldestFirst),
            ],
        }
    }

    /// Total token capacity available for input.
    #[must_use]
    pub fn available_input_tokens(&self) -> usize {
        self.model_context_total
            .saturating_sub(self.output_budget)
            .saturating_sub(self.safety_margin)
    }

    /// Total tokens currently used across all zones.
    #[must_use]
    pub fn total_used_tokens(&self) -> usize {
        self.zones.iter().map(|z| z.used_tokens).sum()
    }

    /// Find a zone by name.
    pub fn zone(&self, name: &str) -> Option<&ContextZone> {
        self.zones.iter().find(|z| z.name == name)
    }

    /// Find a zone by name (mutable).
    pub fn zone_mut(&mut self, name: &str) -> Option<&mut ContextZone> {
        self.zones.iter_mut().find(|z| z.name == name)
    }

    /// Whether the window has overflowed (total used > available).
    #[must_use]
    pub fn is_overflowed(&self) -> bool {
        self.total_used_tokens() > self.available_input_tokens()
    }
}

// ── Compiled context ─────────────────────────────────────────────

/// The result of context compilation — ready for inference.
#[derive(Debug, Clone)]
pub struct CompiledContext {
    /// Ordered items to include in the prompt.
    pub items: Vec<ContextItem>,
    /// Total tokens used.
    pub total_tokens: usize,
    /// Available context budget that was used.
    pub budget: ContextBudget,
    /// Items that were evicted during compilation.
    pub evicted: Vec<ContextItem>,
    /// Items that were summarized/compressed.
    pub summarized: Vec<ContextItem>,
    /// Warnings (e.g. truncation, budget exceeded).
    pub warnings: Vec<String>,
    /// Quality score (0–100).
    pub quality_score: u32,
    /// Whether compilation succeeded.
    pub success: bool,
}

// ── Context compiler ─────────────────────────────────────────────

/// Compiles context items into a final prompt-ready structure.
pub struct ContextCompiler;

impl ContextCompiler {
    /// Compile a context window into an ordered set of items for inference.
    ///
    /// Applies eviction policies per zone, preserves pinned items,
    /// and enforces the overall token budget.
    #[must_use]
    pub fn compile(window: &mut ContextWindow) -> CompiledContext {
        let mut all_items: Vec<ContextItem> = Vec::new();
        let mut evicted = Vec::new();
        let mut warnings = Vec::new();
        let available = window.available_input_tokens();

        // Collect all items zone by zone, applying eviction
        for zone in &mut window.zones {
            // Eviction: if zone exceeds its budget, remove items per policy
            while zone.used_tokens > zone.max_tokens && !zone.items.is_empty() {
                let (idx, item) = Self::select_eviction_target(zone);
                zone.used_tokens = zone.used_tokens.saturating_sub(item.token_count);
                let evicted_item = zone.items.remove(idx);
                evicted.push(evicted_item);
            }

            // Collect remaining items
            for item in &zone.items {
                all_items.push(item.clone());
            }
        }

        // Sort: pinned first, then by authority (system > user > tool > data)
        all_items.sort_by(|a, b| {
            a.pinned
                .cmp(&b.pinned)
                .reverse()
                .then_with(|| a.authority.cmp(&b.authority))
                .then_with(|| b.priority.cmp(&a.priority))
        });

        // Truncate if still over budget
        let mut total: usize = 0;
        let mut final_items = Vec::new();
        let mut overflow_warned = false;

        for item in &all_items {
            if item.pinned && total + item.token_count > available && !overflow_warned {
                warnings.push(format!(
                    "Pinned content ({}) exceeds available context budget ({} used, {} available). System instructions may be truncated.",
                    item.id, total, available
                ));
                overflow_warned = true;
            }
            if total + item.token_count > available && !item.pinned {
                evicted.push(item.clone());
                continue;
            }
            total = total.saturating_add(item.token_count);
            final_items.push(item.clone());
        }

        if total > available && !overflow_warned {
            warnings.push("Context overflow: total content exceeds model capacity".into());
        }

        // Quality score: simple heuristic
        let relevance_sum: u32 = final_items.iter().map(|i| i.relevance).sum();
        let item_count = final_items.len().max(1);
        let quality = (relevance_sum / item_count as u32).min(100);

        // Check for duplicates (exact hash)
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut dedup_count = 0;
        let mut dedup_items = Vec::new();
        for item in &final_items {
            let key = format!("{:?}:{}", item.item_type, item.content);
            let count = seen.entry(key.clone()).or_insert(0);
            if *count > 0 {
                dedup_count += 1;
            } else {
                dedup_items.push(item.clone());
            }
            *count += 1;
        }
        if dedup_count > 0 {
            warnings.push(format!("Removed {dedup_count} duplicate context items"));
        }
        final_items = dedup_items;

        let success =
            total <= available && warnings.iter().all(|w| !w.starts_with("Pinned content"));

        CompiledContext {
            items: final_items,
            total_tokens: total,
            budget: ContextBudget::new(window.model_context_total),
            evicted,
            summarized: Vec::new(),
            warnings,
            quality_score: quality,
            success,
        }
    }

    /// Select which item to evict from a zone.
    fn select_eviction_target(zone: &ContextZone) -> (usize, &ContextItem) {
        // Never evict pinned items
        let candidates: Vec<(usize, &ContextItem)> =
            zone.items.iter().enumerate().filter(|(_, item)| !item.pinned).collect();

        if candidates.is_empty() {
            // If all items are pinned and we must evict, take the last one
            return (zone.items.len() - 1, zone.items.last().unwrap());
        }

        match zone.eviction {
            EvictionPolicy::OldestFirst => candidates.first().map(|(i, item)| (*i, *item)).unwrap(),
            EvictionPolicy::LowestPriority => candidates
                .iter()
                .min_by_key(|(_, item)| item.priority)
                .map(|(i, item)| (*i, *item))
                .unwrap_or(candidates[0]),
            EvictionPolicy::LowestRelevance => candidates
                .iter()
                .min_by_key(|(_, item)| item.relevance)
                .map(|(i, item)| (*i, *item))
                .unwrap_or(candidates[0]),
            EvictionPolicy::NoEviction => {
                // Force evict oldest non-pinned anyway to prevent overflow
                candidates.first().map(|(i, item)| (*i, *item)).unwrap()
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn system_msg(id: &str, text: &str) -> ContextItem {
        ContextItem::new(
            id,
            ContextItemType::SystemInstruction,
            text,
            text.len() / 3,
            Authority::System,
        )
        .pinned()
    }

    fn user_msg(id: &str, text: &str) -> ContextItem {
        ContextItem::new(id, ContextItemType::UserMessage, text, text.len() / 3, Authority::User)
    }

    fn retrieval_doc(id: &str, text: &str, relevance: u32) -> ContextItem {
        ContextItem::new(
            id,
            ContextItemType::RetrievedDocument,
            text,
            text.len() / 3,
            Authority::RetrievedData,
        )
        .with_relevance(relevance)
    }

    #[test]
    fn context_window_default_zones() {
        let window = ContextWindow::new(32768, 4096);
        assert_eq!(window.zones.len(), 4);
        assert!(window.available_input_tokens() < 32768);
        assert!(window.available_input_tokens() > 24000);
    }

    #[test]
    fn zone_can_accept_tokens() {
        let mut zone = ContextZone::new("test", 1000, EvictionPolicy::OldestFirst);
        assert!(zone.can_accept(500));
        zone.used_tokens = 800;
        assert!(zone.can_accept(200));
        assert!(!zone.can_accept(300));
    }

    #[test]
    fn compilation_preserves_pinned() {
        let mut window = ContextWindow::new(1000, 200);
        window
            .zone_mut("system")
            .unwrap()
            .add_item(system_msg("s1", "System instruction that must stay — pinned content test"));
        window.zone_mut("conversation").unwrap().add_item(user_msg("u1", "Hello"));

        let compiled = ContextCompiler::compile(&mut window);
        let has_system = compiled.items.iter().any(|i| i.id == "s1");
        assert!(has_system, "Pinned system instruction must be preserved");
    }

    #[test]
    fn compilation_evicts_old_conversation() {
        let mut window = ContextWindow::new(200, 50);
        // Add a lot of conversation that overflows
        for i in 0..20 {
            window.zone_mut("conversation").unwrap().add_item(user_msg(
                &format!("msg{i}"),
                "This is a conversation message that takes up space",
            ));
        }
        let compiled = ContextCompiler::compile(&mut window);
        assert!(
            !compiled.warnings.is_empty() || compiled.evicted.len() > 0,
            "Overflowed context should evict items or produce warnings"
        );
    }

    #[test]
    fn compilation_detects_duplicates() {
        let mut window = ContextWindow::new(2000, 200);
        let dup_text = "This is duplicate content";
        window.zone_mut("retrieval").unwrap().add_item(retrieval_doc("d1", dup_text, 80));
        window.zone_mut("retrieval").unwrap().add_item(retrieval_doc("d2", dup_text, 85));

        let compiled = ContextCompiler::compile(&mut window);
        let dup_warning = compiled.warnings.iter().any(|w| w.contains("duplicate"));
        assert!(
            dup_warning || compiled.items.iter().filter(|i| i.content == dup_text).count() <= 1,
            "Duplicates should be detected or removed"
        );
    }

    #[test]
    fn eviction_lowest_relevance() {
        let mut zone = ContextZone::new("retrieval", 200, EvictionPolicy::LowestRelevance);
        zone.add_item(retrieval_doc("r1", &"a".repeat(50), 90));
        zone.add_item(retrieval_doc("r2", &"b".repeat(50), 10));
        zone.add_item(retrieval_doc("r3", &"c".repeat(50), 50));

        // Force eviction
        zone.max_tokens = 40; // Only one item fits
        let (idx, item) = ContextCompiler::select_eviction_target(&zone);
        // Lowest relevance item should be selected first
        assert_eq!(item.id, "r2", "Lowest relevance should be evicted first");
    }

    #[test]
    fn authority_ordering() {
        let sys = system_msg("sys", "system");
        let user = user_msg("user", "user");
        let doc = retrieval_doc("doc", "document", 50);
        let mut items = vec![doc.clone(), user.clone(), sys.clone()];
        items.sort_by(|a, b| {
            a.pinned.cmp(&b.pinned).reverse().then_with(|| a.authority.cmp(&b.authority))
        });
        assert_eq!(items[0].id, "sys"); // pinned system first
        assert_eq!(items[1].id, "user"); // user before data
    }

    #[test]
    fn sensitivity_detection() {
        let mut item = ContextItem::new(
            "s",
            ContextItemType::UserMessage,
            "my password is hunter2",
            5,
            Authority::User,
        );
        item.sensitivity = Sensitivity::Secret;
        assert!(item.is_sensitive());
    }

    #[test]
    fn context_window_overflow_detection() {
        let mut window = ContextWindow::new(100, 20);
        window.zone_mut("conversation").unwrap().add_item(user_msg("big", &"x".repeat(300)));
        assert!(window.is_overflowed());
    }
}
