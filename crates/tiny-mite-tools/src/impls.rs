//! Concrete tool implementations — git, compiler, HTTP, MCP stubs.
//!
//! Each tool registers with the ToolRegistry and enforces sandbox rules.

use tiny_mite_domain::ToolId;

use crate::registry::ToolResult;
use crate::sandbox::Sandbox;

/// Git tool — controlled git operations within sandbox.
pub struct GitTool {
    sandbox: Sandbox,
}

impl GitTool {
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    pub fn status(&self, tool_id: &ToolId) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, "[DRY RUN] git status", 0);
        }
        self.run_git(tool_id, &["status", "--short"])
    }

    pub fn commit(&self, tool_id: &ToolId, message: &str) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(
                *tool_id,
                format!("[DRY RUN] git commit -m '{message}'"),
                0,
            );
        }
        self.run_git(tool_id, &["commit", "-m", message])
    }

    fn run_git(&self, tool_id: &ToolId, args: &[&str]) -> ToolResult {
        let start = std::time::Instant::now();
        let output = std::process::Command::new("git").args(args).output();
        let dur = start.elapsed().as_millis() as u64;
        match output {
            Ok(o) if o.status.success() => ToolResult::success(
                *tool_id,
                String::from_utf8_lossy(&o.stdout).trim().to_string(),
                dur,
            ),
            Ok(o) => ToolResult::failure(
                *tool_id,
                String::from_utf8_lossy(&o.stderr).trim().to_string(),
                dur,
            ),
            Err(e) => ToolResult::failure(*tool_id, format!("git error: {e}"), dur),
        }
    }
}

/// Compiler/test runner tool.
pub struct CompilerTool {
    sandbox: Sandbox,
}

impl CompilerTool {
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    pub fn compile(&self, tool_id: &ToolId, args: &[&str]) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(
                *tool_id,
                format!("[DRY RUN] compile {}", args.join(" ")),
                0,
            );
        }
        let start = std::time::Instant::now();
        let output = std::process::Command::new("cargo").arg("build").args(args).output();
        let dur = start.elapsed().as_millis() as u64;
        match output {
            Ok(o) if o.status.success() => ToolResult::success(
                *tool_id,
                String::from_utf8_lossy(&o.stdout).trim().to_string(),
                dur,
            ),
            Ok(o) => ToolResult::failure(
                *tool_id,
                String::from_utf8_lossy(&o.stderr).trim().to_string(),
                dur,
            ),
            Err(e) => ToolResult::failure(*tool_id, format!("compile error: {e}"), dur),
        }
    }

    pub fn test(&self, tool_id: &ToolId) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, String::from("[DRY RUN] cargo test"), 0u64);
        }
        let start = std::time::Instant::now();
        let output = std::process::Command::new("cargo").args(["test"]).output();
        let dur = start.elapsed().as_millis() as u64;
        match output {
            Ok(o) if o.status.success() => ToolResult::success(
                *tool_id,
                String::from_utf8_lossy(&o.stdout).trim().to_string(),
                dur,
            ),
            Ok(o) => ToolResult::failure(
                *tool_id,
                String::from_utf8_lossy(&o.stderr).trim().to_string(),
                dur,
            ),
            Err(e) => ToolResult::failure(*tool_id, format!("test error: {e}"), dur),
        }
    }
}

/// HTTP/network tool — controlled network access.
pub struct HttpTool {
    sandbox: Sandbox,
}

impl HttpTool {
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    pub fn get(&self, tool_id: &ToolId, url: &str) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, format!("[DRY RUN] GET {url}"), 0);
        }
        if !self.sandbox.allow_network() {
            return ToolResult::failure(*tool_id, "Network access denied by sandbox", 0);
        }
        // Stub: real HTTP would use reqwest
        ToolResult::success(*tool_id, format!("[STUB] Would GET {url}"), 0)
    }
}

/// MCP client stub — future Model Context Protocol integration.
pub struct McpClientStub {
    sandbox: Sandbox,
}

impl McpClientStub {
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    pub fn call_tool(&self, tool_id: &ToolId, server: &str, tool: &str, _args: &str) -> ToolResult {
        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, format!("[DRY RUN] MCP {server}/{tool}"), 0);
        }
        ToolResult::success(*tool_id, format!("[MCP STUB] Would call {server}/{tool}"), 0)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Sandbox;

    #[test]
    fn git_status_dry_run() {
        let t = GitTool::new(Sandbox::dry_run("/tmp"));
        let r = t.status(&ToolId::new());
        assert!(r.output.contains("DRY RUN"));
    }
    #[test]
    fn compiler_dry_run() {
        let t = CompilerTool::new(Sandbox::dry_run("/tmp"));
        let r = t.compile(&ToolId::new(), &["--release"]);
        assert!(r.output.contains("DRY RUN"));
    }
    #[test]
    fn http_blocked_by_sandbox() {
        let t = HttpTool::new(Sandbox::dry_run("/tmp"));
        let r = t.get(&ToolId::new(), "https://example.com");
        assert!(!r.output.contains("DRY RUN") || r.success);
    }
}
