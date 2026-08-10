//! Tiny Mite event bus
//!
//! Asynchronous, typed, observable event infrastructure with:
//! - Typed event envelope (ID, correlation, causation, version, timestamp)
//! - Priority-aware publication
//! - Bounded, cancellable subscriptions
//! - Graceful shutdown and subscriber failure isolation
//! - Explicit behavior for buffer exhaustion and duplicate handling
//!
//! The event bus is NOT a logging system. It carries domain events that
//! drive subsystem orchestration.

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![deny(unused_must_use)]

pub mod bus;
pub mod envelope;
pub mod store;

pub use bus::EventBus;
pub use envelope::{Event, EventEnvelope};
pub use store::{
    Checkpoint, EventStore, PruneFilter, QueryFilter, ReplayFilter, SqliteEventStore, StoreError,
};
