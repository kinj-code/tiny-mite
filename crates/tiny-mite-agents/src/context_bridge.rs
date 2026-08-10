//! Context bridge — connects intelligence components to runtime context.
//!
//! Takes TaskAnalysis, Plan, PlanStep, and WorkingMemory and produces
//! context items suitable for the ContextCompiler. This is the primary
//! integration point between the intelligence engine and the model runtime.

use tiny_mite_runtime::context::{Authority, CompiledContext, ContextCompiler};
use tiny_mite_runtime::context::{ContextItem, ContextItemType, ContextWindow, EvictionPolicy};
use tiny_mite_runtime::context::{ContextZone, Sensitivity};
use tiny_mite_runtime::inference::ContextBudget;

use crate::analysis::TaskAnalysis;
use crate::memory::WorkingMemory;
use crate::planner::{Plan, PlanStep};

/// Bridge that assembles context for model inference from intelligence artifacts.
pub struct ContextBridge;

impl ContextBridge {
    /// Build a full context window from a task, plan, step, and working memory.
    ///
    /// Creates zones: system, task, plan, working_memory, retrieval, conversation.
    /// Each zone has a token budget proportional to its priority.
    #[must_use]
    pub fn build(
        task: &TaskAnalysis,
        plan: &Plan,
        current_step: Option<&PlanStep>,
        memory: &WorkingMemory,
        model_context_total: usize,
    ) -> ContextWindow {
        // Reserve tokens for output and tool calls
        let reserved = (model_context_total as f64 * 0.3) as usize;
        let available = model_context_total - reserved;

        // Proportional zone budgets
        let system_budget = (available as f64 * 0.10) as usize; // 10%
        let task_budget = (available as f64 * 0.20) as usize; // 20%
        let plan_budget = (available as f64 * 0.25) as usize; // 25%
        let memory_budget = (available as f64 * 0.20) as usize; // 20%
        let tool_budget = (available as f64 * 0.15) as usize; // 15%
        let conv_budget = (available as f64 * 0.10) as usize; // 10%

        let mut window = ContextWindow::new(model_context_total, reserved);

        // ── System zone (mandatory) ──────────────────────────
        let mut sys_zone = ContextZone::new("system", system_budget, EvictionPolicy::OldestFirst);
        sys_zone.add_item(
            ContextItem::new(
                "system:identity",
                ContextItemType::SystemInstruction,
                "You are Tiny Mite, an offline-first local AI assistant. \
                 You execute tasks using structured tool calls. \
                 Always verify outputs before proceeding.",
                "You are Tiny Mite...".len() / 3,
                Authority::System,
            )
            .pinned()
            .with_priority(100),
        );
        window.zones.push(sys_zone);

        // ── Task zone ────────────────────────────────────────
        let mut task_zone = ContextZone::new("task", task_budget, EvictionPolicy::OldestFirst);
        task_zone.add_item(
            ContextItem::new(
                "task:analysis",
                ContextItemType::Plan,
                format!(
                    "Task: intent={:?}, type={:?}, complexity={:.1}, \
                     requires_planning={}, requires_reasoning={}, \
                     requires_tools={:?}, risk={}",
                    task.intent,
                    task.task_type,
                    task.complexity.overall,
                    task.requires_planning,
                    task.requires_reasoning,
                    task.requires_tools,
                    task.risk_score,
                ),
                "Task: intent=...".len() / 3,
                Authority::System,
            )
            .pinned()
            .with_priority(90),
        );

        // Add tool requirements
        if !task.requires_tools.is_empty() {
            task_zone.add_item(
                ContextItem::new(
                    "task:tools",
                    ContextItemType::ToolCall,
                    format!("Required tools: {}", task.requires_tools.join(", ")),
                    "Required tools: ...".len() / 3,
                    Authority::System,
                )
                .with_priority(85),
            );
        }

        window.zones.push(task_zone);

        // ── Plan zone ────────────────────────────────────────
        let mut plan_zone = ContextZone::new("plan", plan_budget, EvictionPolicy::LowestPriority);
        plan_zone.add_item(
            ContextItem::new(
                "plan:summary",
                ContextItemType::Plan,
                format!(
                    "Task: {}. Plan has {} steps. {} tools required.",
                    plan.task_description,
                    plan.steps.len(),
                    plan.steps.iter().filter(|s| !s.tools.is_empty()).count(),
                ),
                "Task: ... Plan has ... steps.".len() / 3,
                Authority::System,
            )
            .pinned()
            .with_priority(80),
        );

        // Add step descriptions
        for step in &plan.steps {
            let is_current = current_step.map_or(false, |cs| cs.id == step.id);
            plan_zone.add_item(
                ContextItem::new(
                    format!("plan:step:{}", step.id),
                    ContextItemType::Plan,
                    format!(
                        "Step '{}': {} (deps: {} tools: {}) [{}]",
                        step.id,
                        step.description,
                        step.dependencies.len(),
                        step.tools.len(),
                        if is_current { "CURRENT" } else { "pending" }
                    ),
                    "Step: desc...".len() / 3,
                    Authority::System,
                )
                .with_priority(if is_current { 75 } else { 50 }),
            );
        }
        window.zones.push(plan_zone);

        // ── Working memory zone ──────────────────────────────
        let mut mem_zone =
            ContextZone::new("working_memory", memory_budget, EvictionPolicy::LowestPriority);
        let items = memory.items_by_importance();
        for item in items.iter().take(20) {
            mem_zone.add_item(
                ContextItem::new(
                    format!("wm:{}", item.id),
                    ContextItemType::Observation,
                    format!("[{:?}|imp={}] {}", item.category, item.importance, item.content),
                    item.content.len() / 3,
                    Authority::RetrievedData,
                )
                .with_priority(item.importance.min(70)),
            );
        }
        window.zones.push(mem_zone);

        // ── Tool context zone ────────────────────────────────
        let mut tool_zone = ContextZone::new("tools", tool_budget, EvictionPolicy::LowestPriority);
        if let Some(step) = current_step {
            for tool in &step.tools {
                tool_zone.add_item(
                    ContextItem::new(
                        format!("tool:{}", tool),
                        ContextItemType::ToolCall,
                        format!("Available tool: {tool}"),
                        5,
                        Authority::Tool,
                    )
                    .with_priority(60),
                );
            }
        }
        window.zones.push(tool_zone);

        // ── Conversation zone (empty placeholder) ────────────
        let conv_zone = ContextZone::new("conversation", conv_budget, EvictionPolicy::OldestFirst);
        window.zones.push(conv_zone);

        window
    }

    /// Compile a context window and return a ready-to-use compiled context.
    #[must_use]
    pub fn compile(
        task: &TaskAnalysis,
        plan: &Plan,
        current_step: Option<&PlanStep>,
        memory: &WorkingMemory,
        model_context_total: usize,
    ) -> CompiledContext {
        let mut window = Self::build(task, plan, current_step, memory, model_context_total);
        ContextCompiler::compile(&mut window)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{Intent, TaskType};
    use crate::planner::Planner;

    #[test]
    fn builds_context_with_all_zones() {
        let task = TaskAnalysis::simple(Intent::CodeGeneration, TaskType::Implementation);
        let planner = Planner::new();
        let plan = planner.plan(&task, "write a BST");
        let memory = WorkingMemory::new();

        let window = ContextBridge::build(&task, &plan, None, &memory, 8192);

        assert!(window.zone("system").is_some());
        assert!(window.zone("task").is_some());
        assert!(window.zone("plan").is_some());
        assert!(window.zone("working_memory").is_some());
        assert!(window.zone("tools").is_some());
        assert!(window.zone("conversation").is_some());
    }

    #[test]
    fn compile_produces_valid_output() {
        let task = TaskAnalysis::simple(Intent::Debugging, TaskType::BugFix);
        let planner = Planner::new();
        let plan = planner.plan(&task, "fix null pointer");
        let mut memory = WorkingMemory::new();
        memory.load_plan(&plan);

        let compiled = ContextBridge::compile(&task, &plan, None, &memory, 8192);

        assert!(compiled.success);
        assert!(!compiled.items.is_empty());
        assert!(compiled.total_tokens > 0);
        // Should include plan items
        let has_plan = compiled.items.iter().any(|i| i.item_type == ContextItemType::Plan);
        assert!(has_plan);
    }

    #[test]
    fn respects_token_budget() {
        let task = TaskAnalysis::simple(Intent::Question, TaskType::FactualQuery);
        let planner = Planner::new();
        let plan = planner.plan(&task, "what is Rust?");
        let mut memory = WorkingMemory::new();
        for i in 0..50 {
            memory.insert(
                crate::WorkingMemoryItem::new(
                    format!("item_{i}"),
                    crate::memory::MemoryCategory::Fact,
                    format!("memory content {i}"),
                )
                .with_importance((i % 100) as u32),
            );
        }

        let small_budget = 2048;
        let compiled = ContextBridge::compile(&task, &plan, None, &memory, small_budget);

        // Available for input = 2048 * 0.7 ≈ 1433
        assert!(compiled.total_tokens <= 1500, "over budget: {}", compiled.total_tokens);
    }

    #[test]
    fn current_step_gets_higher_priority() {
        let task = TaskAnalysis::simple(Intent::Debugging, TaskType::BugFix);
        let planner = Planner::new();
        let plan = planner.plan(&task, "fix bug");

        let step = plan.steps.first();
        let compiled = ContextBridge::compile(&task, &plan, step, &WorkingMemory::new(), 8192);

        // The current step should be in the items
        let step_id = step.unwrap().id.as_str();
        let step_item = compiled.items.iter().find(|i| i.id == format!("plan:step:{step_id}"));
        assert!(step_item.is_some());
        // Current step should be near the top (after pinned items)
        let pos =
            compiled.items.iter().position(|i| i.id == format!("plan:step:{step_id}")).unwrap();
        assert!(pos < 10, "current step position {pos} should be early");
    }
}
