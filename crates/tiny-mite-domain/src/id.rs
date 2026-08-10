//! Strongly-typed domain identifiers
//!
//! Every entity type receives its own newtype wrapper so that the
//! compiler prevents accidental mixing. Forward and backward
//! (de)serialization uses type-safe formats (e.g. `task_<uuid>`).
//!
//! # Required IDs (per implementation contract 40)
//!
//! `TaskId`, `EventId`, `AgentId`, `ToolId`, `MemoryId`,
//! `DocumentId`, `ModelId`, `ProjectId`, `CorrelationId`

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Macro-generated strongly-typed ID
// ---------------------------------------------------------------------------
macro_rules! typed_id {
    ($name:ident, $prefix:literal) => {
        #[doc = concat!("Strongly-typed identifier for ", stringify!($name))]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            /// Create a new random ID (UUID v4).
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Create from an existing UUID.
            #[must_use]
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Expose the inner UUID.
            #[must_use]
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Expose the inner UUID by value.
            #[must_use]
            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "_{}"), self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.to_string().serialize(s)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                // Accept both plain UUIDs and the prefixed form
                let uuid_str =
                    <&str>::clone(&raw.strip_prefix(concat!($prefix, "_")).unwrap_or(&raw));
                let uuid = Uuid::parse_str(uuid_str).map_err(serde::de::Error::custom)?;
                Ok(Self(uuid))
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = uuid::Error;
            fn try_from(s: &str) -> Result<Self, Self::Error> {
                let s = s.strip_prefix(concat!($prefix, "_")).unwrap_or(s);
                let uuid = Uuid::parse_str(s)?;
                Ok(Self(uuid))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Declare all domain IDs
// ---------------------------------------------------------------------------
typed_id!(TaskId, "task");
typed_id!(EventId, "evt");
typed_id!(AgentId, "agent");
typed_id!(ToolId, "tool");
typed_id!(MemoryId, "mem");
typed_id!(DocumentId, "doc");
typed_id!(ModelId, "model");
typed_id!(ProjectId, "proj");
typed_id!(CorrelationId, "corr");

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_types_are_incompatible() {
        let t = TaskId::new();
        let e = EventId::from_uuid(*t.as_uuid()); // same UUID bytes
        // Despite same bytes they must be different types
        // (compilation would fail if we tried: let x: TaskId = e;)
        assert_ne!(t.to_string(), e.to_string()); // different prefixes
    }

    #[test]
    fn display_includes_prefix() {
        let tid = TaskId::new();
        let s = tid.to_string();
        assert!(s.starts_with("task_"));
        assert_eq!(s.len(), 41); // "task_" + 36-char UUID
    }

    #[test]
    fn roundtrip_json() {
        let original = TaskId::new();
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: TaskId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, parsed);
    }

    #[test]
    fn deserialize_plain_uuid() {
        let uuid = Uuid::new_v4();
        let json = serde_json::to_string(&uuid).expect("serialize uuid");
        let parsed: TaskId = serde_json::from_str(&json).expect("deserialize plain uuid");
        assert_eq!(parsed.as_uuid(), &uuid);
    }
}
