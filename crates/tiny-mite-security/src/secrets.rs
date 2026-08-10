//! Secret store — secure handling of API keys, passwords, and tokens.
//!
//! Secrets are never logged, never serialized to clear text, and must
//! be explicitly exposed when needed.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A securely stored secret value.
///
/// Implements zero-clear-on-drop patterns. Redacts in Debug/Display.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    /// The sensitive value, stored as cleared bytes.
    value: String,
    /// Metadata about this secret (not the value itself).
    label: String,
}

impl Secret {
    /// Create a new secret.
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into() }
    }

    /// Expose the secret value. Caller must handle it securely.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// The label/description of this secret (safe to log).
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret").field("label", &self.label).finish()
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({})", self.label)
    }
}

impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        // Never serialize the actual secret value
        s.serialize_str("***REDACTED***")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Self { value: raw, label: "deserialized_secret".into() })
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Zero out the value for defense-in-depth
        unsafe {
            for byte in self.value.as_bytes_mut() {
                *byte = 0;
            }
        }
    }
}

/// A registry of named secrets.
#[derive(Debug, Default)]
pub struct SecretStore {
    secrets: std::collections::HashMap<String, Secret>,
}

impl SecretStore {
    /// Create a new empty store.
    #[must_use]
    pub fn new() -> Self {
        Self { secrets: std::collections::HashMap::new() }
    }

    /// Store a secret under a key.
    pub fn set(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.secrets.insert(key.into(), Secret::new(value, label));
    }

    /// Get a reference to a secret.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Secret> {
        self.secrets.get(key)
    }

    /// Remove a secret.
    pub fn remove(&mut self, key: &str) -> Option<Secret> {
        self.secrets.remove(key)
    }

    /// Returns number of stored secrets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.secrets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_redacts_in_debug() {
        let s = Secret::new("sk-abc123", "API Key");
        let debug = format!("{s:?}");
        assert!(!debug.contains("sk-abc123"));
        assert!(debug.contains("API Key"));
    }

    #[test]
    fn secret_store_operations() {
        let mut store = SecretStore::new();
        store.set("api_key", "my-key", "Test API Key");
        assert_eq!(store.len(), 1);
        assert!(store.get("api_key").is_some());
        store.remove("api_key");
        assert_eq!(store.len(), 0);
    }
}
