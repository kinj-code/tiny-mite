//! Filesystem tool — controlled file read/write access.
//!
//! All filesystem operations are permission-gated and path-restricted.

use std::path::PathBuf;

use crate::registry::ToolResult;
use crate::sandbox::Sandbox;
use crate::schema::RiskLevel;
use tiny_mite_domain::ToolId;

/// A permission-gated filesystem tool.
pub struct FileSystemTool {
    sandbox: Sandbox,
}

impl FileSystemTool {
    /// Create a new filesystem tool with the given sandbox.
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    /// Read a file within the sandboxed path.
    pub fn read(&self, tool_id: &ToolId, path: &str) -> ToolResult {
        let start = std::time::Instant::now();

        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, format!("[DRY RUN] Would read: {path}"), 0);
        }

        let full_path = match self.sandbox.resolve_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::failure(*tool_id, e, start.elapsed().as_millis() as u64),
        };

        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                ToolResult::success(*tool_id, content, start.elapsed().as_millis() as u64)
            }
            Err(e) => ToolResult::failure(
                *tool_id,
                format!("Failed to read {:?}: {e}", full_path),
                start.elapsed().as_millis() as u64,
            ),
        }
    }

    /// Write content to a file within the sandboxed path.
    pub fn write(&self, tool_id: &ToolId, path: &str, content: &str) -> ToolResult {
        let start = std::time::Instant::now();

        if self.sandbox.is_dry_run() {
            return ToolResult::success(
                *tool_id,
                format!("[DRY RUN] Would write {} bytes to: {path}", content.len()),
                0,
            );
        }

        let full_path = match self.sandbox.resolve_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::failure(*tool_id, e, start.elapsed().as_millis() as u64),
        };

        if let Some(parent) = full_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolResult::failure(
                    *tool_id,
                    format!("Failed to create dir {:?}: {e}", parent),
                    start.elapsed().as_millis() as u64,
                );
            }
        }

        match std::fs::write(&full_path, content) {
            Ok(()) => ToolResult::success(
                *tool_id,
                format!("Wrote {} bytes to {:?}", content.len(), full_path),
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => ToolResult::failure(
                *tool_id,
                format!("Failed to write {:?}: {e}", full_path),
                start.elapsed().as_millis() as u64,
            ),
        }
    }

    /// List files in a sandboxed directory.
    pub fn list(&self, tool_id: &ToolId, path: &str) -> ToolResult {
        let start = std::time::Instant::now();

        if self.sandbox.is_dry_run() {
            return ToolResult::success(*tool_id, format!("[DRY RUN] Would list: {path}"), 0);
        }

        let full_path = match self.sandbox.resolve_path(path) {
            Ok(p) => p,
            Err(e) => return ToolResult::failure(*tool_id, e, start.elapsed().as_millis() as u64),
        };

        match std::fs::read_dir(&full_path) {
            Ok(entries) => {
                let mut listing = String::new();
                for entry in entries.flatten() {
                    listing.push_str(&format!("{}\n", entry.path().display()));
                }
                ToolResult::success(*tool_id, listing, start.elapsed().as_millis() as u64)
            }
            Err(e) => ToolResult::failure(
                *tool_id,
                format!("Failed to list {:?}: {e}", full_path),
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_read_does_not_access_disk() {
        let sandbox = Sandbox::dry_run("/tmp");
        let tool = FileSystemTool::new(sandbox);
        let result = tool.read(&ToolId::new(), "test.txt");
        assert!(result.success);
        assert!(result.output.contains("DRY RUN"));
    }
}
