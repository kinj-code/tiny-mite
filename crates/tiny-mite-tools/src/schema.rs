//! Tool schemas and risk classification.

use serde::{Deserialize, Serialize};

/// Risk level for tool operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// JSON Schema for tool parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub param_type: String,
    pub default: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_levels_have_sensible_names() {
        assert_eq!(format!("{:?}", RiskLevel::None), "None");
        assert_eq!(format!("{:?}", RiskLevel::Critical), "Critical");
    }

    #[test]
    fn schema_serialization() {
        let s = ParameterSchema {
            name: "path".into(),
            description: "File path".into(),
            required: true,
            param_type: "string".into(),
            default: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("path"));
    }
}
