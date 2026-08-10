//! Shell tool — controlled command execution within sandbox.
//!
//! All shell commands pass through the sandbox and permission engine.
//! Model-generated commands are NEVER executed directly.

use std::process::Command;

use crate::registry::ToolResult;
use crate::sandbox::Sandbox;
use tiny_mite_domain::ToolId;

/// A permission-gated shell execution tool.
pub struct ShellTool {
    sandbox: Sandbox,
}

impl ShellTool {
    /// Create a new shell tool with the given sandbox.
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    /// Execute a shell command (if sandbox allows it).
    pub fn execute(&self, tool_id: &ToolId, cmd: &str, args: &[&str]) -> ToolResult {
        let start = std::time::Instant::now();

        if self.sandbox.is_dry_run() {
            return ToolResult::success(
                *tool_id,
                format!("[DRY RUN] Would execute: {cmd} {}", args.join(" ")),
                0,
            );
        }

        if !self.sandbox.allow_shell() {
            return ToolResult::failure(
                *tool_id,
                "Shell execution is not permitted by sandbox",
                start.elapsed().as_millis() as u64,
            );
        }

        let output = match Command::new(cmd).args(args).output() {
            Ok(o) => o,
            Err(e) => {
                return ToolResult::failure(
                    *tool_id,
                    format!("Failed to execute '{cmd}': {e}"),
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let duration = start.elapsed().as_millis() as u64;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            ToolResult::success(*tool_id, stdout.trim().to_string(), duration)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            ToolResult {
                tool_id: *tool_id,
                success: false,
                output: stderr.trim().to_string(),
                error: Some(format!("Exit code: {}", output.status.code().unwrap_or(-1))),
                exit_code: output.status.code(),
                duration_ms: duration,
                cancelled: false,
            }
        }
    }

    /// Check if shell is allowed for this tool.
    #[must_use]
    pub fn is_shell_allowed(&self) -> bool {
        self.sandbox.allow_shell()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_does_not_execute() {
        let sandbox = Sandbox::dry_run("/tmp");
        let tool = ShellTool::new(sandbox);
        let result = tool.execute(&ToolId::new(), "echo", &["hello"]);
        assert!(result.success);
        assert!(result.output.contains("DRY RUN"));
    }

    #[test]
    fn shell_disallowed_when_sandbox_blocks() {
        let sandbox = Sandbox::dry_run("/tmp");
        let tool = ShellTool::new(sandbox);
        assert!(!tool.is_shell_allowed());
    }
}
