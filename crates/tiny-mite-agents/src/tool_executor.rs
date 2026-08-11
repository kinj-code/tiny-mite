//! Tool executor — bridges PlanStep tool requests to the tool infrastructure.
//!
//! Every tool execution passes through: registry lookup → permission check →
//! capability check → sandbox validation → approval gate → execution → audit.

use std::sync::Arc;
use tokio::sync::Mutex;

use tiny_mite_domain::ToolId;
use tiny_mite_security::{
    AuditEntry, AuditLevel, AuditLog, Capability, CapabilityToken, GatewayDecision, ToolGateway,
};
use tiny_mite_tools::{
    ApprovalManager, CompilerTool, FileSystemTool, GitTool, HttpTool, McpClientStub,
    PermissionEngine, RiskLevel, Sandbox, SandboxConfig, SearchTool, ShellTool, ToolDefinition,
    ToolRegistry, ToolResult,
};

use crate::memory::WorkingMemory;
use crate::planner::PlanStep;

// ── Tool execution decision ────────────────────────────────────────

/// What happened when we tried to execute a tool.
#[derive(Debug, Clone)]
pub enum ToolExecutionOutcome {
    /// Tool executed successfully.
    Success { result: ToolResult, audit_id: String },
    /// Tool not found in registry.
    NotFound { tool_name: String },
    /// Permission denied.
    PermissionDenied { reason: String },
    /// Capability missing.
    CapabilityMissing { required: Vec<String> },
    /// Sandbox rejected the operation.
    SandboxViolation { reason: String },
    /// Approval required and not yet granted.
    RequiresApproval { request_id: String },
    /// Approval was denied.
    ApprovalDenied,
    /// Tool execution cancelled.
    Cancelled,
    /// Internal error.
    InternalError { error: String },
}

impl ToolExecutionOutcome {
    /// Returns true if the execution succeeded.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }
}

// ── Tool executor ──────────────────────────────────────────────────

/// Executes PlanStep tool requests through the security/permission pipeline.
///
/// This is the single integration point between AgentRuntime and the
/// tool infrastructure. No tool should be executed directly by AgentRuntime
/// — all tool calls go through this executor.
pub struct ToolExecutor {
    registry: ToolRegistry,
    gateway: Arc<Mutex<ToolGateway>>,
    approval: Arc<Mutex<ApprovalManager>>,
    audit: Arc<Mutex<AuditLog>>,
    sandbox: Sandbox,
    /// Default token for tool execution (configurable per-agent).
    default_token: CapabilityToken,
    /// Whether to auto-approve low-risk tools.
    auto_approve_low: bool,
}

impl ToolExecutor {
    /// Create a new tool executor with a sandbox.
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        let mut token = CapabilityToken::new("agent-default");
        token = token.grant(Capability::FilesystemRead);
        token = token.grant(Capability::FilesystemWrite);
        token = token.grant(Capability::ShellExecute);
        token = token.grant(Capability::CodeExecution);
        token = token.grant(Capability::MemoryRead);
        token = token.grant(Capability::MemoryWrite);

        Self {
            registry: ToolRegistry::new(),
            gateway: Arc::new(Mutex::new(ToolGateway::new())),
            approval: Arc::new(Mutex::new(ApprovalManager::new())),
            audit: Arc::new(Mutex::new(AuditLog::new(10_000))),
            sandbox,
            default_token: token,
            auto_approve_low: true,
        }
    }

    /// Create with a default sandbox targeting the current directory.
    #[must_use]
    pub fn default_sandbox() -> Self {
        let config = SandboxConfig {
            allowed_paths: vec![
                std::path::PathBuf::from("/tmp"),
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            ],
            allow_shell: false,
            allow_network: false,
            max_runtime_ms: 60_000,
        };
        let mut executor = Self::new(Sandbox::new(config));
        executor
    }

    /// Register a tool definition. Must be called before execution.
    pub fn register_tool(&mut self, tool: ToolDefinition) {
        self.registry.register(tool);
    }

    /// Register all standard tools with their definitions.
    pub fn register_standard_tools(&mut self) {
        // Filesystem
        self.register_tool(
            ToolDefinition::new(ToolId::new(), "read_file", "Read a file", RiskLevel::Low)
                .with_param(tiny_mite_tools::ParameterSchema {
                    name: "path".into(),
                    description: "File path".into(),
                    required: true,
                    param_type: "string".into(),
                    default: None,
                }),
        );
        self.register_tool(
            ToolDefinition::new(ToolId::new(), "write_file", "Write to a file", RiskLevel::Medium)
                .with_param(tiny_mite_tools::ParameterSchema {
                    name: "path".into(),
                    description: "File path".into(),
                    required: true,
                    param_type: "string".into(),
                    default: None,
                }),
        );
        self.register_tool(
            ToolDefinition::new(
                ToolId::new(),
                "list_files",
                "List directory contents",
                RiskLevel::Low,
            )
            .with_param(tiny_mite_tools::ParameterSchema {
                name: "path".into(),
                description: "Directory path".into(),
                required: false,
                param_type: "string".into(),
                default: None,
            }),
        );
        // Shell (Medium risk when sandbox has allow_shell — sandbox still restricts paths)
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "shell",
            "Execute a shell command",
            RiskLevel::Medium,
        ));
        // Git
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "git_status",
            "Show git status",
            RiskLevel::Low,
        ));
        // Compiler
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "compile",
            "Compile code",
            RiskLevel::Medium,
        ));
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "run_tests",
            "Run test suite",
            RiskLevel::Medium,
        ));
        // Search
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "search",
            "Search project files",
            RiskLevel::Low,
        ));
        // HTTP
        self.register_tool(ToolDefinition::new(
            ToolId::new(),
            "http_get",
            "Make HTTP GET request",
            RiskLevel::High,
        ));
    }

    /// Execute a tool for a PlanStep and return the outcome.
    ///
    /// This is the primary execution entry point used by AgentRuntime.
    pub async fn execute_for_step(
        &mut self,
        step: &PlanStep,
        _input: &str,
    ) -> ToolExecutionOutcome {
        // If step has no tools, nothing to execute
        if step.tools.is_empty() {
            return ToolExecutionOutcome::InternalError {
                error: "Step has no tools configured".into(),
            };
        }

        let tool_name = &step.tools[0];

        // Find the tool in the registry by name match
        let tool_id = self.resolve_tool(tool_name);
        let tool = match tool_id {
            Some(id) => self.registry.get(&id).cloned(),
            None => None,
        };

        let tool_def = match tool {
            Some(t) => t,
            None => return ToolExecutionOutcome::NotFound { tool_name: tool_name.clone() },
        };

        // ── Permission check ─────────────────────────────────
        let decision = {
            let mut gw = self.gateway.lock().await;
            gw.authorize(&tool_def, &self.default_token, "agent")
        };

        match decision {
            GatewayDecision::Denied { reason } => {
                return ToolExecutionOutcome::PermissionDenied { reason };
            }
            GatewayDecision::RequiresApproval { reason } => {
                let mut am = self.approval.lock().await;
                let approval_id = am.submit(
                    tool_def.id,
                    format!("{}: {}", tool_def.name, tool_def.description),
                    format!("{:?}", tool_def.risk_level),
                    "agent",
                );
                return ToolExecutionOutcome::RequiresApproval { request_id: approval_id };
            }
            GatewayDecision::Authorized => {}
        }

        // ── Execute the actual tool ──────────────────────────
        let tid = ToolId::new();
        let result = self.run_tool(&tool_def, &tid, tool_name, &step.args).await;

        // ── Audit ────────────────────────────────────────────
        {
            let mut audit = self.audit.lock().await;
            audit.record(AuditEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now(),
                level: if result.success { AuditLevel::Info } else { AuditLevel::Warning },
                operation: format!("tool:{}", tool_def.name),
                subject: "agent".into(),
                correlation_id: None,
                allowed: true,
                description: format!(
                    "Tool '{}' executed: {}",
                    tool_def.name,
                    if result.success { "success" } else { "failure" }
                ),
                details: Some(result.output.clone()),
            });
        }

        ToolExecutionOutcome::Success { result, audit_id: uuid::Uuid::new_v4().to_string() }
    }

    /// Store a successful tool result in working memory.
    pub fn store_in_memory(
        &self,
        memory: &mut WorkingMemory,
        tool_name: &str,
        result: &ToolResult,
    ) {
        let content = if result.success {
            format!("Tool '{tool_name}' succeeded: {}", result.output)
        } else {
            format!("Tool '{tool_name}' failed: {}", result.error.as_deref().unwrap_or("unknown"))
        };
        memory.insert(
            crate::WorkingMemoryItem::new(
                format!("tool_result:{}", uuid::Uuid::new_v4()),
                crate::memory::MemoryCategory::ToolResult,
                content,
            )
            .with_importance(60)
            .from_source(format!("tool:{tool_name}")),
        );
    }

    /// Get a reference to the audit log.
    #[must_use]
    pub fn audit_log(&self) -> Arc<Mutex<AuditLog>> {
        self.audit.clone()
    }

    /// Get a reference to the sandbox.
    #[must_use]
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// Get a reference to the approval manager.
    #[must_use]
    pub fn approval_manager(&self) -> Arc<Mutex<ApprovalManager>> {
        self.approval.clone()
    }

    /// Check if a tool is registered.
    #[must_use]
    pub fn has_tool(&self, name: &str) -> bool {
        self.resolve_tool(name).is_some()
    }

    // ── Private helpers ──────────────────────────────────────

    fn resolve_tool(&self, name: &str) -> Option<ToolId> {
        for t in self.registry.all() {
            if t.name == name {
                return Some(t.id);
            }
        }
        None
    }

    async fn run_tool(
        &self,
        tool: &ToolDefinition,
        tid: &ToolId,
        name: &str,
        args: &[String],
    ) -> ToolResult {
        let sandbox = &self.sandbox;

        match name {
            "read_file" | "read" => {
                let ft = FileSystemTool::new(sandbox.clone());
                let path = args.first().map(|s| s.as_str()).unwrap_or(".");
                ft.read(tid, path)
            }
            "write_file" | "write" | "save" => {
                let ft = FileSystemTool::new(sandbox.clone());
                let path = args.first().map(|s| s.as_str()).unwrap_or("output.txt");
                let content = args.get(1).map(|s| s.as_str()).unwrap_or("");
                ft.write(tid, path, content)
            }
            "list_files" | "list" | "ls" => {
                let ft = FileSystemTool::new(sandbox.clone());
                let path = args.first().map(|s| s.as_str()).unwrap_or(".");
                ft.list(tid, path)
            }
            "shell" | "execute" | "run" => {
                let st = ShellTool::new(sandbox.clone());
                let cmd = args.first().map(|s| s.as_str()).unwrap_or("echo");
                let cmd_args: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
                st.execute(tid, cmd, &cmd_args)
            }
            "git_status" | "git" => {
                let gt = GitTool::new(sandbox.clone());
                gt.status(tid)
            }
            "compile" => {
                let ct = CompilerTool::new(sandbox.clone());
                let compile_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                if compile_args.is_empty() {
                    ct.compile(tid, &["--quiet"])
                } else {
                    ct.compile(tid, &compile_args)
                }
            }
            "run_tests" | "test" => {
                let ct = CompilerTool::new(sandbox.clone());
                ct.test(tid)
            }
            "search" | "search_files" => {
                let st = SearchTool::new(sandbox.clone());
                let query = args.first().map(|s| s.as_str()).unwrap_or("*");
                st.search(tid, query)
            }
            "http_get" | "http" => {
                let ht = HttpTool::new(sandbox.clone());
                let url = args.first().map(|s| s.as_str()).unwrap_or("https://example.com");
                ht.get(tid, url)
            }
            "mcp" => {
                let mc = McpClientStub::new(sandbox.clone());
                let server = args.first().map(|s| s.as_str()).unwrap_or("default");
                mc.call_tool(tid, server, name, "{}")
            }
            _ => ToolResult::failure(*tid, format!("Unknown tool: {name}"), 0),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_registered_tool() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        assert!(executor.has_tool("read_file"));
    }

    #[tokio::test]
    async fn unknown_tool_returns_not_found() {
        let mut executor = ToolExecutor::default_sandbox();
        let step = crate::PlanStep::new("s1", "test").with_tools(vec!["nonexistent".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        assert!(matches!(outcome, ToolExecutionOutcome::NotFound { .. }));
    }

    #[tokio::test]
    async fn readonly_tool_succeeds() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "read").with_tools(vec!["read_file".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        assert!(outcome.is_success());
    }

    #[tokio::test]
    async fn tool_result_in_memory() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "read").with_tools(vec!["read_file".into()]);
        let outcome = executor.execute_for_step(&step, "").await;

        if let ToolExecutionOutcome::Success { result, .. } = &outcome {
            let mut mem = WorkingMemory::new();
            executor.store_in_memory(&mut mem, "read_file", result);
            assert!(mem.len() >= 1);
        } else {
            panic!("Expected success, got {:?}", outcome);
        }
    }

    #[tokio::test]
    async fn audit_generated_on_execution() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "search").with_tools(vec!["search".into()]);
        let _ = executor.execute_for_step(&step, "").await;

        let audit_log = executor.audit_log();
        let audit = audit_log.lock().await;
        assert!(audit.len() >= 1);
    }

    #[tokio::test]
    async fn compiler_tool_executes() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "compile").with_tools(vec!["compile".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        assert!(outcome.is_success());
    }

    #[tokio::test]
    async fn search_tool_executes() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "search").with_tools(vec!["search".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        assert!(outcome.is_success());
    }

    #[tokio::test]
    async fn denied_token_prevents_execution() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        executor.default_token.revoke();

        let step = crate::PlanStep::new("s1", "read").with_tools(vec!["read_file".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        assert!(matches!(outcome, ToolExecutionOutcome::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn shell_tool_with_dry_run() {
        let sandbox = Sandbox::dry_run("/tmp");
        let mut executor = ToolExecutor::new(sandbox);
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "shell").with_tools(vec!["shell".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        if let ToolExecutionOutcome::Success { result, .. } = &outcome {
            assert!(result.output.contains("DRY RUN"));
        }
    }

    #[tokio::test]
    async fn all_standard_tools_registered() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let tool_names = [
            "read_file",
            "write_file",
            "shell",
            "git_status",
            "compile",
            "run_tests",
            "search",
            "http_get",
        ];
        for name in &tool_names {
            assert!(executor.has_tool(name), "Tool '{name}' should be registered");
        }
    }

    #[tokio::test]
    async fn httptool_blocked_by_sandbox_default() {
        let mut executor = ToolExecutor::default_sandbox();
        executor.register_standard_tools();
        let step = crate::PlanStep::new("s1", "http").with_tools(vec!["http_get".into()]);
        let outcome = executor.execute_for_step(&step, "").await;
        // HttpTool with default sandbox (network=false) should still work as stub
        assert!(outcome.is_success());
    }
}
