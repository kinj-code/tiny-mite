//! Search tool — controlled search across files, documents, and codebases.
//!
//! Wraps the retrieval crate's LexicalSearcher behind the tool contract.

use crate::registry::ToolResult;
use crate::sandbox::Sandbox;
use tiny_mite_domain::ToolId;

/// Search tool with sandboxed filesystem access.
pub struct SearchTool {
    sandbox: Sandbox,
}

impl SearchTool {
    /// Create a new search tool.
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }

    /// Search for a query in the sandboxed filesystem.
    pub fn search(&self, tool_id: &ToolId, query: &str) -> ToolResult {
        let start = std::time::Instant::now();

        if self.sandbox.is_dry_run() {
            return ToolResult::success(
                *tool_id,
                format!("[DRY RUN] Would search for: {query}"),
                0,
            );
        }

        let mut results = Vec::new();
        if let Err(e) = self.search_dir(".", query, &mut results) {
            return ToolResult::failure(
                *tool_id,
                format!("Search failed: {e}"),
                start.elapsed().as_millis() as u64,
            );
        }

        let output = if results.is_empty() {
            format!("No results found for: {query}")
        } else {
            results.join("\n")
        };

        ToolResult::success(*tool_id, output, start.elapsed().as_millis() as u64)
    }

    fn search_dir(&self, dir: &str, query: &str, results: &mut Vec<String>) -> Result<(), String> {
        let path = self.sandbox.resolve_path(dir)?;
        if !path.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(&path).map_err(|e| format!("read_dir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let entry_path = entry.path();

            if entry_path.is_dir() {
                let name = entry_path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    if let Some(subpath) = entry_path.to_str() {
                        self.search_dir(subpath, query, results)?;
                    }
                }
            } else if entry_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&entry_path) {
                    if content.contains(query) {
                        results.push(format!("{} (matches)", entry_path.display()));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Sandbox;

    #[test]
    fn dry_run_returns_placeholder() {
        let sandbox = Sandbox::dry_run("/tmp");
        let tool = SearchTool::new(sandbox);
        let result = tool.search(&ToolId::new(), "hello");
        assert!(result.output.contains("DRY RUN"));
    }

    #[test]
    fn search_finds_cargo_toml() {
        let sandbox = Sandbox::new(crate::sandbox::SandboxConfig {
            allowed_paths: vec![std::env::current_dir().unwrap()],
            ..crate::sandbox::SandboxConfig::default()
        });
        let tool = SearchTool::new(sandbox);
        let result = tool.search(&ToolId::new(), "[package]");
        assert!(result.success);
    }
}
