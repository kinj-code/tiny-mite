//! Typed event envelope.
//!
//! Every event in the system is wrapped in an [`EventEnvelope`] that carries
//! metadata: unique ID, event type string, version, correlation/causation IDs,
//! timestamp, source component, priority, and a typed payload.
//!
//! Event types are identified by a namespaced string (e.g. `"task.created"`)
//! used for routing and subscription matching.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use tiny_mite_domain::{CorrelationId, EventId, Priority, SecurityContext};

// ---------------------------------------------------------------------------
// Event trait
// ---------------------------------------------------------------------------

/// A domain event that can be sent through the event bus.
pub trait Event: fmt::Debug + Send + Sync + 'static {
    /// The namespaced event type (e.g. `"task.created"`).
    fn event_type(&self) -> &'static str;

    /// The event schema version (used for evolution).
    fn version(&self) -> u32 {
        1
    }
}

// ---------------------------------------------------------------------------
// Event envelope
// ---------------------------------------------------------------------------

/// Metadata wrapper around a domain event.
///
/// The envelope carries routing and tracing information, while the payload
/// contains the domain-specific data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique event identifier.
    pub id: EventId,
    /// Namespaced event type (e.g. `"task.created"`).
    pub event_type: String,
    /// Schema version of the payload.
    pub version: u32,
    /// UTC timestamp of event creation.
    pub timestamp: DateTime<Utc>,
    /// ID of the task/operation that caused this event.
    pub correlation_id: Option<CorrelationId>,
    /// ID of the event that directly caused this event (for event chains).
    pub causation_id: Option<EventId>,
    /// Component that emitted the event.
    pub source: String,
    /// Priority for routing and backpressure decisions.
    pub priority: Priority,
    /// Security context: who/what initiated this.
    pub security: SecurityContext,
    /// The serialized event payload (typically JSON).
    pub payload: serde_json::Value,
    /// Content-type hint for deserialization.
    pub payload_type: String,
}

impl EventEnvelope {
    /// Wrap a typed event into an envelope.
    ///
    /// The payload is serialized to JSON for transport.
    pub fn wrap<E: Event + Serialize>(
        event: &E,
        source: impl Into<String>,
        correlation_id: Option<CorrelationId>,
        causation_id: Option<EventId>,
        security: SecurityContext,
    ) -> Result<Self, serde_json::Error> {
        let payload = serde_json::to_value(event)?;
        Ok(Self {
            id: EventId::new(),
            event_type: event.event_type().to_owned(),
            version: event.version(),
            timestamp: Utc::now(),
            correlation_id,
            causation_id,
            source: source.into(),
            priority: Priority::Normal,
            security,
            payload,
            payload_type: std::any::type_name::<E>().to_owned(),
        })
    }

    /// Wrap with explicit priority override.
    #[must_use]
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Attempt to deserialize the payload back to a typed event.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the payload cannot be deserialized as `E`.
    pub fn unpack<E: Event + for<'de> Deserialize<'de>>(&self) -> Result<E, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }
}

// ---------------------------------------------------------------------------
// Canonical event type constants
// ---------------------------------------------------------------------------

/// Well-known event type strings used across the system.
pub mod event_types {
    /// Task lifecycle events.
    pub mod task {
        /// A new task was received.
        pub const CREATED: &str = "task.created";
        /// Task classification completed.
        pub const CLASSIFIED: &str = "task.classified";
        /// A plan was created for the task.
        pub const PLAN_CREATED: &str = "task.plan_created";
        /// Task is being executed.
        pub const EXECUTING: &str = "task.executing";
        /// Task verification started.
        pub const VERIFYING: &str = "task.verifying";
        /// Task completed successfully.
        pub const COMPLETED: &str = "task.completed";
        /// Task failed.
        pub const FAILED: &str = "task.failed";
        /// Task was cancelled.
        pub const CANCELLED: &str = "task.cancelled";
    }

    /// System lifecycle events.
    pub mod system {
        /// Runtime is starting up.
        pub const STARTING: &str = "system.starting";
        /// Runtime is ready.
        pub const READY: &str = "system.ready";
        /// Runtime is shutting down.
        pub const SHUTTING_DOWN: &str = "system.shutting_down";
        /// Runtime has stopped.
        pub const STOPPED: &str = "system.stopped";
    }

    /// Provider / model events.
    pub mod provider {
        /// A model was loaded.
        pub const MODEL_LOADED: &str = "provider.model_loaded";
        /// A model was unloaded.
        pub const MODEL_UNLOADED: &str = "provider.model_unloaded";
        /// Inference request started.
        pub const INFERENCE_STARTED: &str = "provider.inference_started";
        /// Inference request completed.
        pub const INFERENCE_COMPLETED: &str = "provider.inference_completed";
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestEvent {
        message: String,
    }

    impl Event for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }
    }

    #[test]
    fn envelope_roundtrip() {
        let event = TestEvent { message: "hello".into() };
        let envelope = EventEnvelope::wrap(
            &event,
            "test-component",
            Some(CorrelationId::new()),
            None,
            SecurityContext::default(),
        )
        .expect("wrap");

        assert_eq!(envelope.event_type, "test.event");
        assert_eq!(envelope.version, 1);
        assert!(envelope.correlation_id.is_some());

        let unpacked: TestEvent = envelope.unpack().expect("unpack");
        assert_eq!(unpacked.message, "hello");
    }

    #[test]
    fn priority_override() {
        let event = TestEvent { message: "urgent".into() };
        let envelope = EventEnvelope::wrap(&event, "test", None, None, SecurityContext::default())
            .expect("wrap")
            .with_priority(Priority::High);

        assert_eq!(envelope.priority, Priority::High);
    }

    #[test]
    fn causation_chain() {
        let cause_id = EventId::new();
        let event = TestEvent { message: "caused".into() };
        let envelope =
            EventEnvelope::wrap(&event, "test", None, Some(cause_id), SecurityContext::default())
                .expect("wrap");

        assert_eq!(envelope.causation_id, Some(cause_id));
    }
}
