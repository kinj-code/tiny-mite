//! Tool gateway — permission-aware tool execution boundary.
//!
//! Every tool call passes through the gateway for authorization
//! before execution. The gateway checks capability tokens, risk
//! levels, and produces audit events.

use tiny_mite_tools::registry::{ToolDefinition, ToolResult};
use tiny_mite_tools::schema::RiskLevel;

use crate::audit::{AuditEntry, AuditLevel, AuditLog};
use crate::capability::{Capability, CapabilityToken};
use crate::policy::SecurityPolicy;

/// Result of a gateway authorization check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayDecision {
    /// Tool execution is authorized.
    Authorized,
    /// Tool execution requires user approval.
    RequiresApproval { reason: String },
    /// Tool execution is denied.
    Denied { reason: String },
}

/// The tool gateway — authorization boundary for tool execution.
pub struct ToolGateway {
    policy: SecurityPolicy,
    audit: AuditLog,
}

impl ToolGateway {
    /// Create a new tool gateway.
    #[must_use]
    pub fn new() -> Self {
        Self { policy: SecurityPolicy::new(), audit: AuditLog::new(10_000) }
    }

    /// Authorize a tool execution request.
    pub fn authorize(
        &mut self,
        tool: &ToolDefinition,
        token: &CapabilityToken,
        subject: &str,
    ) -> GatewayDecision {
        // Check token validity
        if !token.is_valid() {
            self.audit_deny(tool, subject, "Token invalid");
            return GatewayDecision::Denied { reason: "Token is invalid".into() };
        }

        // Check risk level
        if risk_level_value(tool.risk_level) > token.max_risk_level {
            self.audit_deny(tool, subject, "Risk level exceeded");
            return GatewayDecision::Denied {
                reason: format!("Risk {:?} exceeds token max", tool.risk_level),
            };
        }

        // Check capabilities
        for cap in &required_caps(tool) {
            if !token.has(*cap) {
                self.audit_deny(tool, subject, &format!("Missing capability: {cap}"));
                return GatewayDecision::Denied { reason: format!("Missing capability: {cap}") };
            }
        }

        // Check approval requirement
        if tool.requires_approval
            && risk_level_value(tool.risk_level) > self.policy.auto_approve_risk_limit
        {
            self.audit_approval(tool, subject);
            return GatewayDecision::RequiresApproval {
                reason: format!("Tool '{}' requires user approval", tool.name),
            };
        }

        self.audit_allow(tool, subject);
        GatewayDecision::Authorized
    }

    /// Get a reference to the audit log.
    #[must_use]
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit
    }

    fn audit_allow(&mut self, tool: &ToolDefinition, subject: &str) {
        self.audit.record(AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            level: AuditLevel::Info,
            operation: format!("tool:{}", tool.name),
            subject: subject.into(),
            correlation_id: None,
            allowed: true,
            description: format!("Authorized: {}", tool.name),
            details: None,
        });
    }

    fn audit_deny(&mut self, tool: &ToolDefinition, subject: &str, reason: &str) {
        self.audit.record(AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            level: AuditLevel::Warning,
            operation: format!("tool:{}", tool.name),
            subject: subject.into(),
            correlation_id: None,
            allowed: false,
            description: format!("Denied: {reason}"),
            details: None,
        });
    }

    fn audit_approval(&mut self, tool: &ToolDefinition, subject: &str) {
        self.audit.record(AuditEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            level: AuditLevel::Info,
            operation: format!("tool:{}", tool.name),
            subject: subject.into(),
            correlation_id: None,
            allowed: false,
            description: format!("Requires approval: {}", tool.name),
            details: None,
        });
    }
}

impl Default for ToolGateway {
    fn default() -> Self {
        Self::new()
    }
}

fn risk_level_value(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::None => 0,
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
    }
}

fn required_caps(tool: &ToolDefinition) -> Vec<Capability> {
    let name = &tool.name;
    if name.contains("compile") || name.contains("run") || name.contains("execute") {
        vec![Capability::CodeExecution, Capability::ShellExecute]
    } else if name.contains("deploy") || name.contains("network") {
        vec![Capability::NetworkAccess]
    } else if name.contains("write") || name.contains("save") {
        vec![Capability::FilesystemRead, Capability::FilesystemWrite]
    } else if name.contains("read") || name.contains("file") {
        vec![Capability::FilesystemRead]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_mite_domain::ToolId;

    #[test]
    fn authorize_read_with_cap() {
        let mut gw = ToolGateway::new();
        let token = CapabilityToken::new("agent").grant(Capability::FilesystemRead);
        let tool = ToolDefinition::new(ToolId::new(), "read_file", "read", RiskLevel::Low);
        assert_eq!(gw.authorize(&tool, &token, "agent"), GatewayDecision::Authorized);
    }

    #[test]
    fn deny_without_cap() {
        let mut gw = ToolGateway::new();
        let token = CapabilityToken::new("agent");
        let tool = ToolDefinition::new(ToolId::new(), "compile", "compiles", RiskLevel::Medium);
        assert!(matches!(gw.authorize(&tool, &token, "agent"), GatewayDecision::Denied { .. }));
    }

    #[test]
    fn require_approval_for_high_risk() {
        let mut gw = ToolGateway::new();
        let token = CapabilityToken::new("agent")
            .grant(Capability::CodeExecution)
            .grant(Capability::ShellExecute);
        let tool = ToolDefinition::new(ToolId::new(), "compile", "compiles", RiskLevel::High);
        assert!(matches!(
            gw.authorize(&tool, &token, "agent"),
            GatewayDecision::RequiresApproval { .. }
        ));
    }
}
