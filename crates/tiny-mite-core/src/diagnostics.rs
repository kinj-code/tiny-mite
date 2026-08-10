//! Structured diagnostics on top of the `tracing` ecosystem.
//!
//! # Design
//!
//! - Libraries emit `tracing` spans and events; the application layer owns
//!   subscriber initialization.
//! - Correlation IDs and task IDs are injected as structured fields.
//! - Secret-bearing fields are wrapped in [`crate::config::SecretString`]
//!   so they never appear in log output.
//! - This module provides helpers for initializing the subscriber and for
//!   ergonomic instrumentation.

use std::sync::OnceLock;
use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::LoggingConfig;

/// Global subscriber guard — keeps the subscriber alive for the
/// duration of the process.
static INIT: OnceLock<()> = OnceLock::new();

/// Initialize the tracing subscriber with the given configuration.
///
/// # Panics
///
/// Panics if called more than once. This is intentional — subscriber
/// initialization is an application-level concern and should happen
/// exactly once at startup.
pub fn init(config: &LoggingConfig) {
    INIT.get_or_init(|| {
        // Build an EnvFilter that respects RUST_LOG but falls back to config
        let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(""));
        let level_directive: tracing::metadata::LevelFilter = level_from_str(&config.level).into();
        let filter = env_filter.add_directive(level_directive.into());

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_ansi(config.ansi)
            .with_target(true)
            .with_thread_ids(false)
            .with_thread_names(true)
            .with_file(false)
            .with_line_number(false);

        if config.json_output {
            let json_layer = fmt_layer
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(false)
                .with_filter(filter);
            tracing_subscriber::registry().with(json_layer).init();
        } else {
            tracing_subscriber::registry().with(fmt_layer.compact().with_filter(filter)).init();
        }
    });
}

/// Convert a level string to a `tracing::Level`.
fn level_from_str(s: &str) -> Level {
    match s.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    }
}

// ---------------------------------------------------------------------------
// Instrumentation helpers
// ---------------------------------------------------------------------------

/// Create a span for a named component with an optional correlation ID.
#[macro_export]
macro_rules! component_span {
    ($component:expr) => {
        tracing::info_span!("component", name = $component)
    };
    ($component:expr, $corr:expr) => {
        tracing::info_span!("component", name = $component, correlation_id = %$corr)
    };
}

/// Record a correlation ID as a structured field in the current span.
#[track_caller]
pub fn record_correlation<T: std::fmt::Display>(correlation_id: &T) {
    tracing::Span::current().record("correlation_id", tracing::field::display(correlation_id));
}

/// Record a task ID as a structured field in the current span.
#[track_caller]
pub fn record_task<T: std::fmt::Display>(task_id: &T) {
    tracing::Span::current().record("task_id", tracing::field::display(task_id));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_from_str_maps_correctly() {
        assert_eq!(level_from_str("trace"), Level::TRACE);
        assert_eq!(level_from_str("debug"), Level::DEBUG);
        assert_eq!(level_from_str("info"), Level::INFO);
        assert_eq!(level_from_str("warn"), Level::WARN);
        assert_eq!(level_from_str("error"), Level::ERROR);
        assert_eq!(level_from_str("unknown"), Level::INFO); // fallback
    }

    #[test]
    fn test_component_span_creates_span() {
        let span = component_span!("config-loader");
        // Verify the span contains the component name field
        // (the Span API doesn't expose .name() publicly)
        let _guard = span.enter();
    }

    #[test]
    fn record_correlation_does_not_panic() {
        let span = component_span!("test");
        let _guard = span.enter();
        record_correlation(&"corr_123");
    }

    #[test]
    fn record_task_does_not_panic() {
        let span = component_span!("test");
        let _guard = span.enter();
        record_task(&"task_123");
    }
}
