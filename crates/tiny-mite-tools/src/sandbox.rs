//! Sandbox — execution isolation and path restriction for tools.
//!
//! Prevents tools from accessing arbitrary filesystem paths, executing
//! arbitrary commands, or making unauthorized network requests.

use std::path::{Path, PathBuf};

/// Sandbox configuration for tool execution.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Allowed filesystem roots (tools cannot escape these).
    pub allowed_paths: Vec<PathBuf>,
    /// Whether to allow shell execution.
    pub allow_shell: bool,
    /// Whether to allow network access.
    pub allow_network: bool,
    /// Maximum execution time in milliseconds.
    pub max_runtime_ms: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_paths: vec![PathBuf::from(".")],
            allow_shell: false,
            allow_network: false,
            max_runtime_ms: 30_000,
        }
    }
}

/// Whether the sandbox is running in dry-run mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunMode {
    /// Tools execute normally.
    Off,
    /// Tools log what they WOULD do but don't execute.
    On,
}

/// Execution sandbox that enforces path restrictions and dry-run mode.
#[derive(Debug, Clone)]
pub struct Sandbox {
    config: SandboxConfig,
    dry_run: DryRunMode,
}

impl Sandbox {
    /// Create a new sandbox with the given config.
    #[must_use]
    pub fn new(config: SandboxConfig) -> Self {
        Self { config, dry_run: DryRunMode::Off }
    }

    /// Create a dry-run sandbox (no actual execution).
    #[must_use]
    pub fn dry_run(root: impl Into<PathBuf>) -> Self {
        Self {
            config: SandboxConfig { allowed_paths: vec![root.into()], ..SandboxConfig::default() },
            dry_run: DryRunMode::On,
        }
    }

    /// Returns true if dry-run mode is active.
    #[must_use]
    pub fn is_dry_run(&self) -> bool {
        self.dry_run == DryRunMode::On
    }

    /// Whether shell execution is permitted.
    #[must_use]
    pub fn allow_shell(&self) -> bool {
        self.config.allow_shell
    }

    /// Whether network access is permitted.
    #[must_use]
    pub fn allow_network(&self) -> bool {
        self.config.allow_network
    }

    /// Resolve and validate a path within the sandbox.
    ///
    /// Returns the canonical path if it stays within allowed roots.
    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = PathBuf::from(path);

        // Resolve relative paths against first allowed root
        let resolved = if candidate.is_relative() {
            self.config.allowed_paths.first().map(|root| root.join(&candidate)).unwrap_or(candidate)
        } else {
            candidate
        };

        // Try canonicalize for existing paths; fall back to parent canonicalize
        // for paths that don't exist yet (e.g. write_file targets)
        let canonical = match std::fs::canonicalize(&resolved) {
            Ok(c) => c,
            Err(_) => {
                // File doesn't exist yet — check parent directory
                if let Some(parent) = resolved.parent() {
                    let parent_canonical = std::fs::canonicalize(parent).map_err(|e| {
                        format!("Parent directory not accessible: {}: {e}", parent.display())
                    })?;
                    parent_canonical.join(resolved.file_name().unwrap_or_default())
                } else {
                    return Err(format!("Path not accessible: {path}"));
                }
            }
        };

        // Verify the canonical path stays within allowed roots
        let allowed = self.config.allowed_paths.iter().any(|root| canonical.starts_with(root));

        if !allowed {
            return Err(format!("Path '{path}' is outside allowed sandbox"));
        }

        Ok(canonical)
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_detected() {
        let sb = Sandbox::dry_run("/tmp");
        assert!(sb.is_dry_run());
    }

    #[test]
    fn relative_path_resolved_within_root() {
        let sb = Sandbox::new(SandboxConfig {
            allowed_paths: vec![std::env::current_dir().unwrap()],
            ..SandboxConfig::default()
        });
        let result = sb.resolve_path("Cargo.toml");
        assert!(result.is_ok());
    }

    #[test]
    fn absolute_path_outside_root_rejected() {
        let sb = Sandbox::new(SandboxConfig {
            allowed_paths: vec![PathBuf::from("/tmp")],
            ..SandboxConfig::default()
        });
        let result = sb.resolve_path("/etc/passwd");
        assert!(result.is_err());
    }
}
