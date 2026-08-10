//! Capability-based security tokens.
//!
//! Capabilities grant specific permissions. No agent can grant itself a
//! capability. Every privileged operation requires an explicit token.

use serde::{Deserialize, Serialize};
use std::fmt;
use tiny_mite_domain::CorrelationId;

/// A specific permission that can be granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Read files within allowed paths.
    FilesystemRead,
    /// Write files within allowed paths.
    FilesystemWrite,
    /// Execute shell commands (subject to sandbox).
    ShellExecute,
    /// Access the network.
    NetworkAccess,
    /// Execute compiled/test code.
    CodeExecution,
    /// Read from memory store.
    MemoryRead,
    /// Write to memory store.
    MemoryWrite,
    /// Retrieve documents.
    RetrievalAccess,
    /// Invoke model inference.
    ModelInference,
    /// Register new tools.
    ToolRegistration,
    /// Modify security policy.
    PolicyModification,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FilesystemRead => write!(f, "filesystem:read"),
            Self::FilesystemWrite => write!(f, "filesystem:write"),
            Self::ShellExecute => write!(f, "shell:execute"),
            Self::NetworkAccess => write!(f, "network:access"),
            Self::CodeExecution => write!(f, "code:execute"),
            Self::MemoryRead => write!(f, "memory:read"),
            Self::MemoryWrite => write!(f, "memory:write"),
            Self::RetrievalAccess => write!(f, "retrieval:access"),
            Self::ModelInference => write!(f, "model:inference"),
            Self::ToolRegistration => write!(f, "tool:register"),
            Self::PolicyModification => write!(f, "policy:modify"),
        }
    }
}

/// A token granting a set of capabilities to a subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique token identifier.
    pub id: String,
    /// Which subject holds this token.
    pub subject: String,
    /// The capabilities granted.
    pub capabilities: Vec<Capability>,
    /// Correlation ID for tracing token usage.
    pub correlation_id: CorrelationId,
    /// When the token expires (None = never).
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Maximum risk level permitted.
    pub max_risk_level: u8,
    /// Whether this token is currently valid.
    pub is_active: bool,
}

impl CapabilityToken {
    /// Create a new token with no capabilities.
    #[must_use]
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            subject: subject.into(),
            capabilities: Vec::new(),
            correlation_id: CorrelationId::new(),
            expires_at: None,
            max_risk_level: 3,
            is_active: true,
        }
    }

    /// Grant a capability.
    #[must_use]
    pub fn grant(mut self, cap: Capability) -> Self {
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
        self
    }

    /// Check if this token has a specific capability.
    #[must_use]
    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Check if the token is currently valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.is_active && self.expires_at.map_or(true, |t| chrono::Utc::now() < t)
    }

    /// Revoke the token.
    pub fn revoke(&mut self) {
        self.is_active = false;
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_grants_and_checks_capability() {
        let token = CapabilityToken::new("agent-1").grant(Capability::FilesystemRead);
        assert!(token.has(Capability::FilesystemRead));
        assert!(!token.has(Capability::ShellExecute));
    }

    #[test]
    fn token_is_valid_by_default() {
        let token = CapabilityToken::new("agent-2");
        assert!(token.is_valid());
    }

    #[test]
    fn revoked_token_is_invalid() {
        let mut token = CapabilityToken::new("agent-3");
        token.revoke();
        assert!(!token.is_valid());
    }

    #[test]
    fn capability_display_format() {
        assert_eq!(Capability::FilesystemRead.to_string(), "filesystem:read");
        assert_eq!(Capability::ShellExecute.to_string(), "shell:execute");
    }
}
