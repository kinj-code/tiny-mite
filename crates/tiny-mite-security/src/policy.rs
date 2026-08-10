//! Security policies — access control rules for Tiny Mite.
//!
//! Policies determine what capabilities are required for specific
//! operations and enforce project boundaries, risk limits, and
//! path restrictions.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::capability::{Capability, CapabilityToken};

/// Access control policy for a specific resource or operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// What this policy protects.
    pub resource: String,
    /// Required capabilities to access this resource.
    pub required_caps: Vec<Capability>,
    /// Whether user approval is required.
    pub require_approval: bool,
    /// Allowed filesystem paths (empty = all allowed).
    pub allowed_paths: Vec<String>,
    /// Whether this policy is currently active.
    pub active: bool,
}

impl AccessPolicy {
    /// Create a policy requiring specific capabilities.
    #[must_use]
    pub fn requires(resource: impl Into<String>, caps: Vec<Capability>) -> Self {
        Self {
            resource: resource.into(),
            required_caps: caps,
            require_approval: false,
            allowed_paths: Vec::new(),
            active: true,
        }
    }

    /// Check if a token satisfies this policy.
    #[must_use]
    pub fn is_satisfied_by(&self, token: &CapabilityToken) -> bool {
        if !self.active {
            return true;
        }
        if !token.is_valid() {
            return false;
        }
        self.required_caps.iter().all(|cap| token.has(*cap))
    }
}

/// The security policy for Tiny Mite.
///
/// Aggregates multiple access policies, path restrictions, and
/// capability requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// Access policies indexed by resource name.
    pub policies: HashMap<String, AccessPolicy>,
    /// Default required capabilities for network access.
    pub network_requires: Vec<Capability>,
    /// Default required capabilities for filesystem access.
    pub filesystem_requires: Vec<Capability>,
    /// Default required capabilities for shell execution.
    pub shell_requires: Vec<Capability>,
    /// Maximum risk level permitted without explicit approval.
    pub auto_approve_risk_limit: u8,
    /// Whether prompt-injection defenses are enabled.
    pub prompt_injection_defense: bool,
}

impl SecurityPolicy {
    /// Create a default security policy with conservative defaults.
    #[must_use]
    pub fn new() -> Self {
        let mut policies = HashMap::new();
        policies.insert(
            "filesystem:read".into(),
            AccessPolicy::requires("filesystem:read", vec![Capability::FilesystemRead]),
        );
        policies.insert(
            "filesystem:write".into(),
            AccessPolicy::requires(
                "filesystem:write",
                vec![Capability::FilesystemRead, Capability::FilesystemWrite],
            ),
        );
        policies.insert(
            "shell:execute".into(),
            AccessPolicy::requires("shell:execute", vec![Capability::ShellExecute]),
        );
        policies.insert(
            "network:access".into(),
            AccessPolicy::requires("network:access", vec![Capability::NetworkAccess]),
        );

        Self {
            policies,
            network_requires: vec![Capability::NetworkAccess],
            filesystem_requires: vec![Capability::FilesystemRead],
            shell_requires: vec![Capability::ShellExecute],
            auto_approve_risk_limit: 2,
            prompt_injection_defense: true,
        }
    }

    /// Check if a token is authorized for a resource.
    #[must_use]
    pub fn can_access(&self, resource: &str, token: &CapabilityToken) -> bool {
        if let Some(policy) = self.policies.get(resource) {
            policy.is_satisfied_by(token)
        } else {
            // Default: deny — no policy means no access
            false
        }
    }

    /// Add an access policy.
    pub fn add_policy(&mut self, resource: impl Into<String>, policy: AccessPolicy) {
        self.policies.insert(resource.into(), policy);
    }

    /// Remove an access policy.
    pub fn remove_policy(&mut self, resource: &str) {
        self.policies.remove(resource);
    }
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_satisfied_by_token_with_capabilities() {
        let policy = AccessPolicy::requires("test", vec![Capability::FilesystemRead]);
        let token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
        assert!(policy.is_satisfied_by(&token));
    }

    #[test]
    fn policy_denied_without_capability() {
        let policy = AccessPolicy::requires("test", vec![Capability::ShellExecute]);
        let token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
        assert!(!policy.is_satisfied_by(&token));
    }

    #[test]
    fn revoked_token_is_denied() {
        let policy = AccessPolicy::requires("test", vec![Capability::FilesystemRead]);
        let mut token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
        token.revoke();
        assert!(!policy.is_satisfied_by(&token));
    }

    #[test]
    fn default_policy_requires_caps() {
        let policy = SecurityPolicy::new();
        let token = CapabilityToken::new("agent");
        assert!(!policy.can_access("shell:execute", &token));
    }

    #[test]
    fn token_with_caps_can_access() {
        let policy = SecurityPolicy::new();
        let token = CapabilityToken::new("agent").grant(Capability::ShellExecute);
        assert!(policy.can_access("shell:execute", &token));
    }
}
