//! Error taxonomy integration for the core runtime.
//!
//! Re-exports and extends the domain error types from `tiny-mite-domain`.
//! Provides conversion traits and helpers for propagating structured errors
//! through the runtime layers.
//!
//! # Rule
//!
//! All privileged operations return `DomainError`. This module provides
//! ergonomic helpers for creating and propagating those errors, but the
//! canonical error type remains in `tiny-mite-domain`.

pub use tiny_mite_domain::{CorrelationId, DomainError, ErrorCategory, RetryPolicy};

/// Extension trait for `Result` types to add correlation context.
pub trait ResultExt<T> {
    /// Attach a correlation ID to any `Err` variant.
    fn with_correlation(self, cid: CorrelationId) -> Result<T, DomainError>;
}

impl<T> ResultExt<T> for Result<T, DomainError> {
    fn with_correlation(self, cid: CorrelationId) -> Result<T, DomainError> {
        self.map_err(|e| e.with_correlation(cid))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_ext_adds_correlation() {
        let cid = CorrelationId::new();
        let err = DomainError::timeout("timeout").with_correlation(cid);
        assert_eq!(err.correlation_id, Some(cid));
    }

    #[test]
    fn result_ext_preserves_ok() {
        let result: Result<i32, DomainError> = Ok(42);
        let cid = CorrelationId::new();
        assert_eq!(result.with_correlation(cid).unwrap(), 42);
    }

    #[test]
    fn known_error_categories_are_mapped() {
        // Verify that the domain error categories are accessible
        let err = DomainError::invalid_input("bad input");
        assert_eq!(err.category, ErrorCategory::InvalidInput);
        assert_eq!(err.retry, RetryPolicy::NonRetryable);
    }
}
