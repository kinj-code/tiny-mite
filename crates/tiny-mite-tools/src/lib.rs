//! Tiny Mite tool system: tool registry, schemas, permission gateway, concrete tools.
//!
//! Tools are the interface between agents and the outside world.
//! Every tool has an explicit contract: input schema, output schema,
//! risk level, resource limits, and cancellation support.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod approval;
pub mod filesystem;
pub mod impls;
pub mod permission;
pub mod registry;
pub mod sandbox;
pub mod schema;
pub mod search_tool;
pub mod shell;

pub use approval::ApprovalManager;
pub use filesystem::FileSystemTool;
pub use impls::{CompilerTool, GitTool, HttpTool, McpClientStub};
pub use permission::PermissionEngine;
pub use registry::{ToolDefinition, ToolRegistry, ToolResult};
pub use sandbox::{DryRunMode, Sandbox, SandboxConfig};
pub use schema::{ParameterSchema, RiskLevel};
pub use search_tool::SearchTool;
pub use shell::ShellTool;
