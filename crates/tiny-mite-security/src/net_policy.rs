//! Network policy — controls what network access tools and agents have.
//!
//! All network access is denied by default. Explicit capabilities and
//! policies are required to permit any outbound connection.

use serde::{Deserialize, Serialize};

/// Network access policy for Tiny Mite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    /// Whether any network access is allowed.
    pub allow_network: bool,
    /// Allowed hosts (empty = any if allow_network is true).
    pub allowed_hosts: Vec<String>,
    /// Allowed ports (empty = any).
    pub allowed_ports: Vec<u16>,
    /// Whether to require explicit approval for each network request.
    pub require_approval: bool,
    /// Maximum response size in bytes.
    pub max_response_size: usize,
    /// Default timeout in milliseconds.
    pub default_timeout_ms: u64,
}

impl NetworkPolicy {
    /// Create the most restrictive default policy (no network access).
    #[must_use]
    pub fn default_deny() -> Self {
        Self {
            allow_network: false,
            allowed_hosts: Vec::new(),
            allowed_ports: Vec::new(),
            require_approval: true,
            max_response_size: 10_485_760, // 10 MB
            default_timeout_ms: 30_000,
        }
    }

    /// Check if a host:port combination is allowed.
    #[must_use]
    pub fn is_allowed(&self, host: &str, port: u16) -> bool {
        if !self.allow_network {
            return false;
        }
        let host_ok =
            self.allowed_hosts.is_empty() || self.allowed_hosts.iter().any(|h| host.contains(h));
        let port_ok = self.allowed_ports.is_empty() || self.allowed_ports.contains(&port);
        host_ok && port_ok
    }
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self::default_deny()
    }
}

/// Filesystem access policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    /// Whether any filesystem access is allowed.
    pub allow_fs: bool,
    /// Allowed read paths.
    pub read_paths: Vec<String>,
    /// Allowed write paths.
    pub write_paths: Vec<String>,
    /// Maximum file size for read operations.
    pub max_read_size: usize,
    /// Whether to require approval for writes.
    pub require_write_approval: bool,
}

impl FilesystemPolicy {
    /// Create the default restrictive policy.
    #[must_use]
    pub fn default_deny() -> Self {
        Self {
            allow_fs: false,
            read_paths: vec![".".into()],
            write_paths: Vec::new(),
            max_read_size: 104_857_600, // 100 MB
            require_write_approval: true,
        }
    }
}

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self::default_deny()
    }
}

/// Policy against memory poisoning (data injected into context to manipulate agents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPoisoningDefense {
    /// Whether memory poisoning defenses are active.
    pub enabled: bool,
    /// Whether to detect known injection patterns in memory content.
    pub detect_injection: bool,
    /// Whether to track provenance of all memory items.
    pub track_provenance: bool,
    /// Maximum age of unverified memory before it's flagged.
    pub max_unverified_age_seconds: u64,
}

impl MemoryPoisoningDefense {
    /// Create the default defense configuration.
    #[must_use]
    pub fn default_defense() -> Self {
        Self {
            enabled: true,
            detect_injection: true,
            track_provenance: true,
            max_unverified_age_seconds: 3600,
        }
    }
}

impl Default for MemoryPoisoningDefense {
    fn default() -> Self {
        Self::default_defense()
    }
}

// ── Tests ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_default_deny() {
        let policy = NetworkPolicy::default_deny();
        assert!(!policy.allow_network);
        assert!(!policy.is_allowed("example.com", 443));
    }

    #[test]
    fn filesystem_default_restrictive() {
        let policy = FilesystemPolicy::default_deny();
        assert!(!policy.allow_fs);
        assert!(policy.require_write_approval);
    }

    #[test]
    fn memory_poisoning_defaults() {
        let defense = MemoryPoisoningDefense::default_defense();
        assert!(defense.enabled);
        assert!(defense.track_provenance);
    }
}
