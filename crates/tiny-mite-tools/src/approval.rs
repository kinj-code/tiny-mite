//! Approval manager — user-approval gating for high-risk tool operations.
//!
//! The approval manager queues tool execution requests that require
//! explicit user consent and provides a backend API for the UI to
//! approve or deny them.

use std::collections::HashMap;
use tiny_mite_domain::ToolId;

/// State of a pending approval request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalState {
    /// Waiting for user decision.
    Pending,
    /// User approved the request.
    Approved,
    /// User denied the request.
    Denied,
    /// The request timed out.
    Timeout,
}

/// A single pending approval.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    /// Unique request identifier.
    pub id: String,
    /// Tool being requested.
    pub tool_id: ToolId,
    /// Human-readable description of what will be done.
    pub description: String,
    /// Risk level of the requested operation.
    pub risk_level: String,
    /// Subject requesting the operation.
    pub subject: String,
    /// Current state.
    pub state: ApprovalState,
    /// When the request was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Backend manager for tool execution approvals.
///
/// Provides an API for the UI to list pending approvals and
/// approve/deny them. Does NOT implement the UI itself.
pub struct ApprovalManager {
    /// Pending and resolved approval requests.
    requests: HashMap<String, ApprovalRequest>,
    /// Whether auto-approval is enabled for low-risk operations.
    auto_approve_low_risk: bool,
}

impl ApprovalManager {
    /// Create a new approval manager.
    #[must_use]
    pub fn new() -> Self {
        Self { requests: HashMap::new(), auto_approve_low_risk: true }
    }

    /// Submit a tool execution for approval.
    pub fn submit(
        &mut self,
        tool_id: ToolId,
        description: impl Into<String>,
        risk_level: impl Into<String>,
        subject: impl Into<String>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let request = ApprovalRequest {
            id: id.clone(),
            tool_id,
            description: description.into(),
            risk_level: risk_level.into(),
            subject: subject.into(),
            state: ApprovalState::Pending,
            created_at: chrono::Utc::now(),
        };
        self.requests.insert(id.clone(), request);
        id
    }

    /// Approve a pending request.
    pub fn approve(&mut self, id: &str) -> Option<&ApprovalRequest> {
        if let Some(req) = self.requests.get_mut(id) {
            req.state = ApprovalState::Approved;
        }
        self.requests.get(id)
    }

    /// Deny a pending request.
    pub fn deny(&mut self, id: &str) -> Option<&ApprovalRequest> {
        if let Some(req) = self.requests.get_mut(id) {
            req.state = ApprovalState::Denied;
        }
        self.requests.get(id)
    }

    /// List all pending requests.
    #[must_use]
    pub fn pending(&self) -> Vec<&ApprovalRequest> {
        self.requests.values().filter(|r| r.state == ApprovalState::Pending).collect()
    }

    /// Look up a request by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ApprovalRequest> {
        self.requests.get(id)
    }

    /// Returns the number of pending approvals.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending().len()
    }

    /// Whether auto-approval is enabled for low-risk operations.
    #[must_use]
    pub fn auto_approve_low_risk(&self) -> bool {
        self.auto_approve_low_risk
    }

    /// Set auto-approval for low-risk operations.
    pub fn set_auto_approve(&mut self, enabled: bool) {
        self.auto_approve_low_risk = enabled;
    }
}

impl Default for ApprovalManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_and_approve() {
        let mut mgr = ApprovalManager::new();
        let id = mgr.submit(ToolId::new(), "delete file", "high", "agent");
        assert_eq!(mgr.get(&id).unwrap().state, ApprovalState::Pending);
        assert_eq!(mgr.pending_count(), 1);

        mgr.approve(&id);
        let updated = mgr.get(&id).unwrap();
        assert_eq!(updated.state, ApprovalState::Approved);
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn deny_request() {
        let mut mgr = ApprovalManager::new();
        let id = mgr.submit(ToolId::new(), "run shell command", "critical", "agent");
        mgr.deny(&id);
        assert_eq!(mgr.get(&id).unwrap().state, ApprovalState::Denied);
    }

    #[test]
    fn auto_approve_default() {
        let mgr = ApprovalManager::new();
        assert!(mgr.auto_approve_low_risk());
    }
}
