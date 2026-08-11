//! Tiny Mite agent runtime: intelligence orchestration layer.
//!
//! Provides the intelligence components that make small local models
//! behave substantially more intelligently through architecture rather
//! than parameter count.
//!
//! # Architecture
//!
//! ```text
//! IntentClassifier → TaskAnalysis → TaskComplexityEstimator → Planner
//!                                                                  ↓
//!                                                           Plan + Steps
//!                                                                  ↓
//!                                                          ContextEngine
//!                                                                  ↓
//!                                                          IntelligenceLoop
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod analysis;
pub mod complexity;
pub mod context_bridge;
pub mod intent;
pub mod memory;
pub mod memory_types;
pub mod planner;
pub mod reflection;
pub mod registry;
pub mod repair;
pub mod runtime;
pub mod tool_executor;
pub mod tool_parser;
pub mod validator;
pub mod verifier;

pub use analysis::TaskAnalysis;
pub use complexity::{ComplexityScore, TaskComplexityEstimator};
pub use intent::{Intent, IntentClassifier, TaskType};
pub use memory::{WorkingMemory, WorkingMemoryItem, WorkingMemorySnapshot};
pub use memory_types::{
    ConsolidationCriteria, ConsolidationResult, EpisodicMemory, ProceduralMemory, ProjectMemory,
    SemanticMemory,
};
pub use planner::{ExecutionPolicy, Plan, PlanStep, Planner, RetryPolicy, VerificationPolicy};
pub use reflection::{Reflection, ReflectionResult};
pub use registry::{AgentDefinition, AgentRegistry, AgentState};
pub use repair::RepairLoop;
pub use runtime::{AgentConversation, AgentLoopConfig, AgentRuntime, ConversationMessage, TaskResult};
pub use tool_parser::{parse_tool_calls, ParsedToolCall};
pub use tool_executor::{ToolExecutionOutcome, ToolExecutor};
pub use validator::{PlanValidator, ValidationResult};
pub use verifier::VerificationEngine;
