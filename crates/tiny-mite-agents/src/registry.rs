//! Agent registry — manages available agent definitions and their lifecycle.
//!
//! The registry tracks which agents exist, their capabilities, and their
//! current state. Agents are task-oriented workers activated when needed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_mite_domain::AgentId;

/// The state of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    /// Agent is idle, available for tasks.
    Idle,
    /// Agent is actively working on a task.
    Busy,
    /// Agent is paused.
    Paused,
    /// Agent encountered an error.
    Error,
    /// Agent is stopped.
    Stopped,
}

/// Defines what tasks an agent can handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique agent identifier.
    pub id: AgentId,
    /// Human-readable name.
    pub name: String,
    /// Agent role description.
    pub role: String,
    /// Capabilities this agent provides.
    pub capabilities: Vec<String>,
    /// Current state.
    pub state: AgentState,
    /// Whether this agent can be self-activated.
    pub can_self_activate: bool,
    /// Maximum concurrent tasks.
    pub max_concurrent_tasks: usize,
}

impl AgentDefinition {
    /// Create a new agent definition.
    #[must_use]
    pub fn new(id: AgentId, name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            role: role.into(),
            capabilities: Vec::new(),
            state: AgentState::Idle,
            can_self_activate: false,
            max_concurrent_tasks: 1,
        }
    }
}

/// Registry for agent definitions.
#[derive(Debug, Default)]
pub struct AgentRegistry {
    agents: HashMap<AgentId, AgentDefinition>,
}

impl AgentRegistry {
    /// Create a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self { agents: HashMap::new() }
    }

    /// Register an agent.
    pub fn register(&mut self, agent: AgentDefinition) {
        self.agents.insert(agent.id, agent);
    }

    /// Get an agent by ID.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<&AgentDefinition> {
        self.agents.get(id)
    }

    /// List all agents matching a capability.
    #[must_use]
    pub fn find_by_capability(&self, capability: &str) -> Vec<&AgentDefinition> {
        self.agents.values().filter(|a| a.capabilities.contains(&capability.to_owned())).collect()
    }

    /// List all idle agents.
    #[must_use]
    pub fn idle_agents(&self) -> Vec<&AgentDefinition> {
        self.agents.values().filter(|a| a.state == AgentState::Idle).collect()
    }

    /// Update agent state.
    pub fn set_state(&mut self, id: &AgentId, state: AgentState) {
        if let Some(agent) = self.agents.get_mut(id) {
            agent.state = state;
        }
    }

    /// Returns the number of registered agents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Returns true if no agents registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_retrieve() {
        let mut reg = AgentRegistry::new();
        let agent = AgentDefinition::new(AgentId::new(), "coder", "writes code");
        reg.register(agent.clone());
        assert_eq!(reg.len(), 1);
        assert!(reg.get(&agent.id).is_some());
    }

    #[test]
    fn find_by_capability() {
        let mut reg = AgentRegistry::new();
        let mut agent = AgentDefinition::new(AgentId::new(), "coder", "writes code");
        agent.capabilities = vec!["code_generation".into()];
        reg.register(agent);
        assert_eq!(reg.find_by_capability("code_generation").len(), 1);
    }

    #[test]
    fn idle_agents_filter() {
        let mut reg = AgentRegistry::new();
        let mut agent = AgentDefinition::new(AgentId::new(), "coder", "writes code");
        agent.state = AgentState::Idle;
        reg.register(agent);
        assert_eq!(reg.idle_agents().len(), 1);
    }
}
