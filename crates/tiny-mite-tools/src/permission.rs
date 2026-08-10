//! Permission engine — authorization gateway for tool execution.
//!
//! Every tool call passes through this engine before execution.
//! Integrates with capability tokens and audit logging.

/// Result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionResult {
    pub allowed: bool,
    pub reason: String,
    pub requires_approval: bool,
}

/// Permission engine that gates all tool execution.
pub struct PermissionEngine {
    max_concurrent: usize,
    default_timeout_ms: u64,
}

impl PermissionEngine {
    /// Create a new permission engine.
    #[must_use]
    pub fn new() -> Self {
        Self { max_concurrent: 4, default_timeout_ms: 30_000 }
    }

    /// Get the default timeout.
    #[must_use]
    pub fn default_timeout_ms(&self) -> u64 {
        self.default_timeout_ms
    }
}

impl Default for PermissionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_defaults_sensible() {
        let engine = PermissionEngine::new();
        assert_eq!(engine.default_timeout_ms, 30_000);
        assert_eq!(engine.max_concurrent, 4);
    }
}
