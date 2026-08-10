//! Structured error taxonomy
//!
//! Every privileged operation returns a `DomainError` that carries:
//! - an error category (for routing/retry/logic);
//! - an optional user-facing message;
//! - a correlation ID for tracing;
//! - whether the error is retryable.

use std::fmt;

use crate::id::CorrelationId;

// ---------------------------------------------------------------------------
// Error category
// ---------------------------------------------------------------------------

/// Coarse-grained classification used by callers for retry / escalation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// The request was invalid (fix the caller).
    InvalidInput,
    /// The requested resource was not found.
    NotFound,
    /// The operation is not permitted (policy / capability).
    Unauthorized,
    /// A transient infrastructure failure (retry may succeed).
    Transient,
    /// A permanent infrastructure failure (do not retry without change).
    Permanent,
    /// The operation timed out.
    Timeout,
    /// The operation was cancelled.
    Cancelled,
    /// An upstream provider returned an error.
    Upstream,
    /// An unexpected internal invariant was violated.
    Internal,
}

// ---------------------------------------------------------------------------
// Retry policy
// ---------------------------------------------------------------------------

/// Guidance for callers that implement retry loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetryPolicy {
    /// Safe to retry with backoff.
    Retryable,
    /// Do not retry without changing the request.
    NonRetryable,
    /// The request was cancelled — do not retry.
    Cancelled,
}

impl From<ErrorCategory> for RetryPolicy {
    fn from(cat: ErrorCategory) -> Self {
        match cat {
            ErrorCategory::Transient | ErrorCategory::Timeout => Self::Retryable,
            ErrorCategory::Cancelled => Self::Cancelled,
            _ => Self::NonRetryable,
        }
    }
}

// ---------------------------------------------------------------------------
// Domain error
// ---------------------------------------------------------------------------

/// The canonical error type returned by all privileged operations.
///
/// # Fields
///
/// - `category`: machine-readable classification.
/// - `retry`: derived from `category`, exposed for convenience.
/// - `correlation_id`: binds the error to a request/task.
/// - `message`: a developer-oriented message (may be displayed).
/// - `user_action`: optional guidance for a non-technical user.
/// - `source`: the underlying error, if any.
#[derive(Debug)]
pub struct DomainError {
    pub category: ErrorCategory,
    pub retry: RetryPolicy,
    pub correlation_id: Option<CorrelationId>,
    pub message: String,
    pub user_action: Option<String>,
    pub source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl DomainError {
    /// Create a new domain error with the given category and message.
    #[must_use]
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        let retry = RetryPolicy::from(category);
        Self {
            category,
            retry,
            correlation_id: None,
            message: message.into(),
            user_action: None,
            source: None,
        }
    }

    /// Attach a correlation ID for tracing.
    #[must_use]
    pub fn with_correlation(mut self, cid: CorrelationId) -> Self {
        self.correlation_id = Some(cid);
        self
    }

    /// Suggest a user-facing recovery action.
    #[must_use]
    pub fn with_user_action(mut self, action: impl Into<String>) -> Self {
        self.user_action = Some(action.into());
        self
    }

    /// Attach the underlying source error.
    #[must_use]
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Convenience: input validation error.
    #[must_use]
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::InvalidInput, msg)
    }

    /// Convenience: not-found error.
    #[must_use]
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::new(ErrorCategory::NotFound, format!("Resource not found: {}", resource.into()))
    }

    /// Convenience: unauthorized error.
    #[must_use]
    pub fn unauthorized(operation: impl Into<String>) -> Self {
        Self::new(
            ErrorCategory::Unauthorized,
            format!("Operation not permitted: {}", operation.into()),
        )
    }

    /// Convenience: transient infrastructure error.
    #[must_use]
    pub fn transient(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Transient, msg)
    }

    /// Convenience: permanent infrastructure error.
    #[must_use]
    pub fn permanent(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Permanent, msg)
    }

    /// Convenience: timeout.
    #[must_use]
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Timeout, msg)
    }

    /// Convenience: cancelled.
    #[must_use]
    pub fn cancelled(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Cancelled, msg)
    }

    /// Convenience: upstream provider error.
    #[must_use]
    pub fn upstream(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Upstream, msg)
    }

    /// Convenience: internal invariant violation.
    #[must_use]
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCategory::Internal, msg)
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.category, self.message)?;
        if let Some(cid) = &self.correlation_id {
            write!(f, " (correlation: {cid})")?;
        }
        if let Some(src) = &self.source {
            write!(f, " | caused by: {src}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DomainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_is_retryable() {
        let err = DomainError::transient("network timeout");
        assert_eq!(err.retry, RetryPolicy::Retryable);
    }

    #[test]
    fn invalid_input_is_non_retryable() {
        let err = DomainError::invalid_input("missing field 'goal'");
        assert_eq!(err.retry, RetryPolicy::NonRetryable);
    }

    #[test]
    fn display_includes_category_and_message() {
        let err = DomainError::not_found("model xyz");
        let s = err.to_string();
        assert!(s.contains("[NotFound]"));
        assert!(s.contains("model xyz"));
    }

    #[test]
    fn display_includes_correlation_when_set() {
        let cid = CorrelationId::new();
        let err = DomainError::timeout("inference timeout").with_correlation(cid);
        let s = err.to_string();
        assert!(s.contains(&cid.to_string()));
    }

    #[test]
    fn source_chain() {
        let inner = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err = DomainError::upstream("ollama unavailable").with_source(inner);
        assert!(std::error::Error::source(&err).is_some());
    }
}
