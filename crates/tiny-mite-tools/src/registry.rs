//! Tool registry — manages available tools with explicit contracts.
//!
//! Every tool must declare its input schema, output schema, risk level,
//! resource limits, timeout, and cancellation support. Tool execution is
//! auditable and permission-gated.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_mite_domain::ToolId;

use crate::schema::{ParameterSchema, RiskLevel};

// ── Tool definition ───────────────────────────────────────────────

/// A registered tool with its full contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique tool identifier.
    pub id: ToolId,
    /// Human-readable name.
    pub name: String,
    /// Description of what this tool does.
    pub description: String,
    /// Risk level for safety classification.
    pub risk_level: RiskLevel,
    /// Input parameters the tool accepts.
    pub input_schema: Vec<ParameterSchema>,
    /// Expected output format description.
    pub output_description: String,
    /// Whether this tool supports cancellation.
    pub supports_cancellation: bool,
    /// Maximum execution time in milliseconds (0 = no limit).
    pub timeout_ms: u64,
    /// Whether this tool requires user approval.
    pub requires_approval: bool,
    /// Capabilities this tool provides (for agent matching).
    pub capabilities: Vec<String>,
}

impl ToolDefinition {
    /// Create a new tool definition with sensible defaults.
    #[must_use]
    pub fn new(
        id: ToolId,
        name: impl Into<String>,
        description: impl Into<String>,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            description: description.into(),
            risk_level,
            input_schema: Vec::new(),
            output_description: String::new(),
            supports_cancellation: false,
            timeout_ms: 30000,
            requires_approval: matches!(risk_level, RiskLevel::High | RiskLevel::Critical),
            capabilities: Vec::new(),
        }
    }

    /// Add an input parameter.
    #[must_use]
    pub fn with_param(mut self, param: ParameterSchema) -> Self {
        self.input_schema.push(param);
        self
    }

    /// Set cancellation support.
    #[must_use]
    pub fn with_cancellation(mut self, enabled: bool) -> Self {
        self.supports_cancellation = enabled;
        self
    }

    /// Set capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }
}

// ── Tool result ───────────────────────────────────────────────────

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The tool that was executed.
    pub tool_id: ToolId,
    /// Whether execution succeeded.
    pub success: bool,
    /// Output data (may be empty on failure).
    pub output: String,
    /// Error message if execution failed.
    pub error: Option<String>,
    /// Exit code (if applicable).
    pub exit_code: Option<i32>,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the tool was cancelled.
    pub cancelled: bool,
}

impl ToolResult {
    /// Create a successful result.
    #[must_use]
    pub fn success(tool_id: ToolId, output: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_id,
            success: true,
            output: output.into(),
            error: None,
            exit_code: Some(0),
            duration_ms,
            cancelled: false,
        }
    }

    /// Create a failure result.
    #[must_use]
    pub fn failure(tool_id: ToolId, error: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            tool_id,
            success: false,
            output: String::new(),
            error: Some(error.into()),
            exit_code: Some(1),
            duration_ms,
            cancelled: false,
        }
    }

    /// Create a cancelled result.
    #[must_use]
    pub fn cancelled(tool_id: ToolId, duration_ms: u64) -> Self {
        Self {
            tool_id,
            success: false,
            output: String::new(),
            error: Some("Cancelled".into()),
            exit_code: None,
            duration_ms,
            cancelled: true,
        }
    }
}

// ── Tool registry ─────────────────────────────────────────────────

/// Manages available tool definitions.
///
/// Tools are registered by their ToolId and looked up when an agent
/// requests execution. The registry does NOT execute tools — that
/// belongs to the Tool Gateway / Permission Engine.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    /// All registered tools.
    tools: HashMap<ToolId, ToolDefinition>,
}

impl ToolRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    /// Register a tool definition.
    pub fn register(&mut self, tool: ToolDefinition) {
        self.tools.insert(tool.id, tool);
    }

    /// Look up a tool by ID.
    #[must_use]
    pub fn get(&self, id: &ToolId) -> Option<&ToolDefinition> {
        self.tools.get(id)
    }

    /// List all registered tool IDs.
    #[must_use]
    pub fn ids(&self) -> Vec<&ToolId> {
        self.tools.keys().collect()
    }

    /// List all registered tools.
    #[must_use]
    pub fn all(&self) -> Vec<&ToolDefinition> {
        self.tools.values().collect()
    }

    /// Find tools by capability.
    #[must_use]
    pub fn find_by_capability(&self, capability: &str) -> Vec<&ToolDefinition> {
        self.tools.values().filter(|t| t.capabilities.contains(&capability.to_owned())).collect()
    }

    /// Returns the number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns `true` if no tools are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Remove a tool from the registry.
    pub fn unregister(&mut self, id: &ToolId) -> Option<ToolDefinition> {
        self.tools.remove(id)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_retrieve() {
        let mut registry = ToolRegistry::new();
        let tool = ToolDefinition::new(ToolId::new(), "read_file", "Reads a file", RiskLevel::Low);
        registry.register(tool.clone());
        assert_eq!(registry.len(), 1);
        assert!(registry.get(&tool.id).is_some());
    }

    #[test]
    fn find_by_capability() {
        let mut registry = ToolRegistry::new();
        let tool =
            ToolDefinition::new(ToolId::new(), "compile", "Compiles code", RiskLevel::Medium)
                .with_capabilities(vec!["code_execution".into()]);
        registry.register(tool);
        let found = registry.find_by_capability("code_execution");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn tool_result_success_has_output() {
        let result = ToolResult::success(ToolId::new(), "hello", 100);
        assert!(result.success);
        assert_eq!(result.output, "hello");
    }

    #[test]
    fn tool_result_failure_has_error() {
        let result = ToolResult::failure(ToolId::new(), "permission denied", 50);
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("permission denied"));
    }

    #[test]
    fn high_risk_requires_approval() {
        let tool = ToolDefinition::new(
            ToolId::new(),
            "deploy",
            "Deploys to production",
            RiskLevel::Critical,
        );
        assert!(tool.requires_approval);
    }

    #[test]
    fn low_risk_no_approval() {
        let tool = ToolDefinition::new(ToolId::new(), "read", "Reads a file", RiskLevel::Low);
        assert!(!tool.requires_approval);
    }
}
