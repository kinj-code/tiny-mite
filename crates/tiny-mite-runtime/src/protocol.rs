//! Phase 12 — Model Tool Protocol Configuration
//!
//! Configures how Tiny Mite communicates its tool protocol to small models.
//! Different models respond better to different formats (JSON, XML, examples).

/// The tool protocol format to present to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProtocolFormat {
    /// JSON object: {"tool":"name","arguments":{...}}
    Json,
    /// OpenAI function-calling: [{"name":"tool","arguments":{...}}]
    OpenAi,
    /// XML: <tool_call><name>tool</name><args>[...]</args></tool_call>
    Xml,
    /// Compact JSON (no whitespace)
    CompactJson,
}

/// Configuration for how Tiny Mite presents tools to a specific model.
#[derive(Debug, Clone)]
pub struct ModelToolProtocolConfig {
    /// Which format the model prefers.
    pub preferred_format: ToolProtocolFormat,
    /// Fallback formats if the preferred fails.
    pub fallback_formats: Vec<ToolProtocolFormat>,
    /// Custom system prompt (replaces default).
    pub system_prompt: Option<String>,
    /// Maximum tool calls per model response.
    pub max_tool_calls_per_response: usize,
    /// Whether to allow markdown-fenced JSON.
    pub allow_markdown: bool,
    /// Whether to allow XML format.
    pub allow_xml: bool,
    /// Whether to attempt fuzzy repair on parse failure.
    pub allow_repair: bool,
    /// Repair prompt sent when parsing fails.
    pub repair_prompt: Option<String>,
    /// Whether to strictly enforce the preferred format.
    pub strict_format: bool,
    /// Model name pattern this config applies to.
    pub model_pattern: String,
}

impl Default for ModelToolProtocolConfig {
    fn default() -> Self {
        Self {
            preferred_format: ToolProtocolFormat::OpenAi,
            fallback_formats: vec![ToolProtocolFormat::Json],
            system_prompt: None,
            max_tool_calls_per_response: 1,
            allow_markdown: true,
            allow_xml: true,
            allow_repair: true,
            repair_prompt: Some(
                "Your tool call could not be parsed.\n\
                 Use: [{\"name\":\"TOOL\",\"arguments\":{\"arg\":\"val\"}}]".into(),
            ),
            strict_format: false,
            model_pattern: "*".into(),
        }
    }
}

impl ModelToolProtocolConfig {
    /// Minimal prompt for small models — short, constrained instruction.
    pub fn minimal() -> Self {
        Self {
            system_prompt: Some(
                "You have access to tools. When a tool is needed, output ONLY:\n\
                 [{\"name\":\"tool\",\"arguments\":{\"param\":\"value\"}}]\n\
                 Available: write_file(path,content) read_file(path) shell(cmd) run_tests search(query) list_files(path)".into(),
            ),
            allow_markdown: false,
            allow_xml: false,
            strict_format: true,
            repair_prompt: Some("Invalid format. Output ONLY: [{\"name\":\"tool\",\"arguments\":{...}}]".into()),
            ..Default::default()
        }
    }

    /// JSON with explicit one-shot example.
    pub fn json_with_example() -> Self {
        Self {
            system_prompt: Some(
                "Tools: write_file(path,content) read_file(path) shell(cmd) run_tests\n\n\
                 Example:\n\
                 User: Create /tmp/x.txt with content hello\n\
                 Assistant: [{\"name\":\"write_file\",\"arguments\":{\"path\":\"/tmp/x.txt\",\"content\":\"hello\"}}]\n\n\
                 Output ONLY the JSON array. No markdown, no explanation.".into(),
            ),
            ..Default::default()
        }
    }

    /// Returns true if this config matches the given model name.
    pub fn matches_model(&self, model_name: &str) -> bool {
        if self.model_pattern == "*" {
            return true;
        }
        model_name.contains(&self.model_pattern)
    }
}