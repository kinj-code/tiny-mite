//! Domain value objects
//!
//! Reusable value types: task priority, event priority, security context,
//! task status, and execution budgets.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------

/// Priority level used for tasks and events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Best-effort background work.
    Low,
    /// Standard priority.
    Normal,
    /// Elevated priority.
    High,
    /// Urgent — may preempt other work.
    Critical,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Normal => write!(f, "normal"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// ---------------------------------------------------------------------------
// Security context (event envelope)
// ---------------------------------------------------------------------------

/// Who or what initiated an event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Subject {
    /// Human user.
    User,
    /// System service.
    System,
    /// An autonomous agent.
    Agent(String), // Agent identifier
}

/// The security scope an event operates within.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecurityScope {
    /// Limited to the current project.
    Project,
    /// Limited to the current workspace.
    Workspace,
    /// System-wide.
    System,
}

/// Security annotation attached to every event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecurityContext {
    pub subject: Subject,
    pub scope: SecurityScope,
}

impl Default for SecurityContext {
    fn default() -> Self {
        Self { subject: Subject::User, scope: SecurityScope::Project }
    }
}

// ---------------------------------------------------------------------------
// Task status
// ---------------------------------------------------------------------------

/// The task state machine (per implementation contract 42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    /// Initial state.
    New,
    /// Classifying task complexity/intent.
    Classifying,
    /// Building an execution plan.
    Planning,
    /// Gathering relevant context.
    ContextPreparing,
    /// Actively executing.
    Executing,
    /// Verifying outputs.
    Verifying,
    /// Reflecting on results.
    Reflecting,
    /// Updating memory.
    MemoryUpdate,
    /// Repairing after verification failure.
    Repairing,
    /// Successfully completed.
    Complete,
    /// Cancelled by user or system.
    Cancelled,
    /// Blocked on external dependency.
    Blocked,
}

impl TaskStatus {
    /// Returns `true` if the status is an active (non-terminal) state.
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::New
                | Self::Classifying
                | Self::Planning
                | Self::ContextPreparing
                | Self::Executing
                | Self::Verifying
                | Self::Reflecting
                | Self::MemoryUpdate
                | Self::Repairing
        )
    }

    /// Returns `true` if the status is terminal.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Cancelled | Self::Blocked)
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::New => "NEW",
            Self::Classifying => "CLASSIFYING",
            Self::Planning => "PLANNING",
            Self::ContextPreparing => "CONTEXT_PREPARING",
            Self::Executing => "EXECUTING",
            Self::Verifying => "VERIFYING",
            Self::Reflecting => "REFLECTING",
            Self::MemoryUpdate => "MEMORY_UPDATE",
            Self::Repairing => "REPAIRING",
            Self::Complete => "COMPLETE",
            Self::Cancelled => "CANCELLED",
            Self::Blocked => "BLOCKED",
        };
        write!(f, "{s}")
    }
}

// ---------------------------------------------------------------------------
// Resource budget
// ---------------------------------------------------------------------------

/// Bounded resource envelope for a single task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// Maximum wall-clock duration.
    pub max_duration: Duration,
    /// Maximum number of inference calls.
    pub max_inference_steps: usize,
    /// Maximum number of tool calls.
    pub max_tool_calls: usize,
    /// Maximum total tokens (input + output) across all steps.
    pub max_total_tokens: usize,
    /// Maximum retry attempts.
    pub max_retries: usize,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_duration: Duration::from_secs(300),
            max_inference_steps: 50,
            max_tool_calls: 30,
            max_total_tokens: 128_000,
            max_retries: 3,
        }
    }
}

impl ResourceBudget {
    /// Validate that at least one resource headroom remains.
    #[must_use]
    pub fn is_exhausted(
        &self,
        elapsed: Duration,
        inference_steps: usize,
        tool_calls: usize,
        total_tokens: usize,
        retries: usize,
    ) -> bool {
        elapsed >= self.max_duration
            || inference_steps >= self.max_inference_steps
            || tool_calls >= self.max_tool_calls
            || total_tokens >= self.max_total_tokens
            || retries >= self.max_retries
    }
}

// ---------------------------------------------------------------------------
// Execution constraints
// ---------------------------------------------------------------------------

/// User-supplied or system-supplied constraints for a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionConstraints {
    /// Required tool capabilities (none = no tool access).
    pub required_tools: Vec<String>,
    /// Filesystem path restrictions (none = no FS access).
    pub allowed_paths: Vec<String>,
    /// Maximum risk level permitted for tools.
    pub max_risk_level: RiskLevel,
    /// Whether to allow network access.
    pub allow_network: bool,
}

impl Default for ExecutionConstraints {
    fn default() -> Self {
        Self {
            required_tools: Vec::new(),
            allowed_paths: Vec::new(),
            max_risk_level: RiskLevel::Low,
            allow_network: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Risk level
// ---------------------------------------------------------------------------

/// Coarse tool risk classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Read-only, no side effects.
    None,
    /// Local read operations.
    Low,
    /// Local write operations.
    Medium,
    /// Network or process creation.
    High,
    /// Destructive or privileged.
    Critical,
}

// ---------------------------------------------------------------------------
// Agent input/output contracts (per doc 41)
// ---------------------------------------------------------------------------

/// Agent input — the contract an agent receives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    pub task_id: crate::TaskId,
    pub role: String,
    pub goal: String,
    pub constraints: Vec<String>,
    pub context_refs: Vec<crate::MemoryId>,
    pub capability_refs: Vec<String>,
    pub resource_budget: ResourceBudget,
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
}

/// Agent output — what an agent produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    pub status: TaskStatus,
    pub artifacts: Vec<serde_json::Value>,
    pub proposed_actions: Vec<ToolActionProposal>,
    pub evidence: Vec<serde_json::Value>,
    pub lessons: Vec<String>,
    pub next_state: Option<String>,
}

/// A tool action proposed by an agent (subject to gateway authorization).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolActionProposal {
    pub tool_id: crate::ToolId,
    pub action: String,
    pub parameters: serde_json::Value,
    pub justification: String,
    pub estimated_risk: RiskLevel,
}

// ---------------------------------------------------------------------------
// Provider identity and capabilities (per doc 41)
// ---------------------------------------------------------------------------

/// A provider identity descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderIdentity {
    pub name: String,
    pub version: String,
    pub provider_type: String,
}

/// Capabilities a provider may advertise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub text_generation: bool,
    pub streaming: bool,
    pub embeddings: bool,
    pub reranking: bool,
    pub vision: bool,
    pub audio: bool,
    pub grammar: bool,
    pub structured_output: bool,
    pub tool_calling: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_active_vs_terminal() {
        assert!(TaskStatus::Executing.is_active());
        assert!(!TaskStatus::Complete.is_active());
        assert!(TaskStatus::Complete.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(TaskStatus::Blocked.is_terminal());
    }

    #[test]
    fn resource_budget_exhaustion() {
        let budget = ResourceBudget::default();
        assert!(!budget.is_exhausted(Duration::ZERO, 0, 0, 0, 0));
        assert!(budget.is_exhausted(Duration::from_secs(301), 0, 0, 0, 0));
        assert!(!budget.is_exhausted(Duration::from_secs(200), 10, 5, 50000, 1));
        assert!(budget.is_exhausted(Duration::from_secs(200), 50, 5, 50000, 1)); // max steps hit
    }

    #[test]
    fn security_context_default() {
        let ctx = SecurityContext::default();
        assert_eq!(ctx.subject, Subject::User);
        assert_eq!(ctx.scope, SecurityScope::Project);
    }

    #[test]
    fn priority_ordering() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Normal);
        assert!(Priority::Normal > Priority::Low);
    }
}
