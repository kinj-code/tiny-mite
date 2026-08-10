//! Tiny Mite scheduler: resource management, task scheduling, and backpressure
//!
//! # Task Registry
//!
//! The authoritative local registry of all tasks: active, completed, failed,
//! cancelled, and recoverable work.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod cancellation;
pub mod scheduler;
pub mod task_registry;

pub use cancellation::{CancelReason, CancellationManager, CancellationToken};
pub use scheduler::{
    HardwareProfile, PressureLevel, ResourceManager, ResourceReservation, Scheduler,
    SchedulingDecision,
};
pub use task_registry::{TaskRecord, TaskRegistry};
