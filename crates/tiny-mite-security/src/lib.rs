//! Tiny Mite security: capability tokens, sandbox, prompt-injection defenses.
//!
//! Security is a first-class subsystem. All tool output, model output,
//! retrieved documents, and external content are treated as untrusted.

#![warn(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod audit;
pub mod capability;
pub mod gateway;
pub mod net_policy;
pub mod policy;
pub mod secrets;
pub mod security_tests;
pub mod validation;

pub use audit::{AuditEntry, AuditLevel, AuditLog};
pub use capability::{Capability, CapabilityToken};
pub use gateway::{GatewayDecision, ToolGateway};
pub use net_policy::{FilesystemPolicy, MemoryPoisoningDefense, NetworkPolicy};
pub use policy::{AccessPolicy, SecurityPolicy};
pub use secrets::{Secret, SecretStore};
pub use validation::OutputValidator;
