//! Tiny Mite domain types
//!
//! Core domain primitives including strongly-typed identifiers,
//! structured error taxonomy, and value objects used across subsystems.
//!
//! # Design principle
//!
//! IDs are newtype wrappers with `'static` names that prevent
//! accidental confusion at compile time. Every privileged operation
//! returns a structured `DomainError` with metadata suitable for
//! observability, retry decisions, and user-facing messaging.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod error;
pub mod id;
pub mod values;

// Re-exports for convenience
pub use error::{DomainError, ErrorCategory, RetryPolicy};
pub use id::*;
pub use values::*;
