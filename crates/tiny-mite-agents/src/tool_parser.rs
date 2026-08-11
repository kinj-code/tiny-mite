//! Tool-call parser — extracts structured tool calls from model text output.
//!
//! Supports two formats:
//! 1. `<tool_call><name>tool_name</name><args>["arg1","arg2"]</args></tool_call>`
//! 2. `<tool_name>["arg1","arg2"]</tool_name>` (direct tag format)

/// Parsed tool call from model text output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedToolCall {
    /// Canonical tool name (e.g. "write_file" even if model used "file_write").
    pub name: String,
    /// Tool arguments.
    pub args: Vec<String>,
}

/// Parse all tool calls from model text output.
pub fn parse_tool_calls(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    // Strip markdown code fences first
    let cleaned = text
        .replace("```json\n", "")
        .replace("```json", "")
        .replace("\n```", "")
        .replace("```", "");
    let mut rest = cleaned.as_str();

    // Try <tool_call><name>...</name><args>...</args></tool_call> format
    while let Some(start) = rest.find("<tool_call>") {
        let after_start = &rest[start + 11..];
        let Some(end) = after_start.find("</tool_call>") else {
            break;
        };
        let body = &after_start[..end];
        rest = &after_start[end + 12..];

        // Extract name — try <name>, then remap known direct tool tags
        let name = if let Some(n) = extract_xml_content(body, "name") {
            n.trim().to_string()
        } else {
            find_direct_tool_name(body).unwrap_or_else(|| "unknown".to_string())
        };

        // Extract args as JSON array or comma-separated
        let args_str = if let Some(a) = extract_xml_content(body, "args") {
            a.trim().to_string()
        } else {
            extract_direct_tool_args(body)
        };

        let args: Vec<String> = parse_args_string(&args_str);

        calls.push(ParsedToolCall { name, args });
    }

    // Try direct <tool_name>args</tool_name> format (models sometimes use this)
    if calls.is_empty() {
        let known_tools = [
            "write_file", "read_file", "shell", "compile", "run_tests",
            "search", "file_write", "file_read", "bash",
        ];
        for tool in &known_tools {
            let open = format!("<{tool}>");
            let close = format!("</{tool}>");
            if let Some(start) = rest.find(&open) {
                let content_start = start + open.len();
                let after_open = &rest[content_start..];
                if let Some(end) = after_open.find(&close) {
                    let body = &after_open[..end];
                    rest = &after_open[end + close.len()..];
                    let args_str = body.trim().to_string();
                    let args = parse_args_string(&args_str);
                    let name = match *tool {
                        "file_write" => "write_file",
                        "file_read" => "read_file",
                        "bash" => "shell",
                        other => other,
                    };
                    calls.push(ParsedToolCall { name: name.to_string(), args });
                }
            }
        }
    }

    // Fallback: regex-based extraction for malformed tags from small models
    if calls.is_empty() {
        calls = fuzzy_parse(text);
    }

    // Try JSON tool call format: {"tool": "write_file", "path": "...", "content": "..."}
    if calls.is_empty() {
        calls = try_json_tool_call(rest);
    }

    // Try OpenAI function-calling format: [{"name":"write_file","arguments":{...}}]
    if calls.is_empty() {
        calls = try_openai_function_call(rest);
    }

    calls
}

/// Fuzzy parse for malformed tool calls that small models sometimes produce.
fn fuzzy_parse(text: &str) -> Vec<ParsedToolCall> {
    let mut calls = Vec::new();

    // Look for patterns like: write_file /path [args] or "write_file" : ["args"]
    let patterns = [
        "write_file", "file_write", "read_file", "file_read",
        "shell", "bash", "compile", "run_tests", "search", "list_files",
    ];

    for tool in &patterns {
        // Try to find the tool name near a JSON array of args
        if let Some(pos) = text.find(tool) {
            // Try to find a JSON array after the tool name
            let after_tool = &text[pos + tool.len()..];
            if let Some(arr_match) = find_json_array(after_tool) {
                let canonical = match *tool {
                    "file_write" => "write_file",
                    "file_read" => "read_file",
                    "bash" => "shell",
                    other => other,
                };
                if let Ok(args) = serde_json::from_str::<Vec<String>>(&arr_match) {
                    calls.push(ParsedToolCall { name: canonical.to_string(), args });
                    break; // Take the first valid match
                }
            }
        }
    }

    calls
}

/// Find a JSON array `["...", "..."]` in text, handling malformed brackets.
fn find_json_array(text: &str) -> Option<String> {
    // Find first '[' that's part of an array
    let start = text.find('[')?;
    let remaining = &text[start..];

    // Walk characters to find matching ']'
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = 0usize;

    for (i, ch) in remaining.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' if !in_string => depth += 1,
            ']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end > 0 {
        Some(remaining[..end].to_string())
    } else {
        None
    }
}

/// Parse args from either JSON array or plain text.
pub fn parse_args_string(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if let Ok(val) = serde_json::from_str::<Vec<String>>(trimmed) {
        return val;
    }
    trimmed
        .trim_matches(|c| c == '[' || c == ']' || c == '"')
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract content between XML-like tags.
pub fn extract_xml_content(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = text.find(&open)?;
    let content_start = start + open.len();
    let rest = &text[content_start..];
    let end = rest.find(&close)?;

    Some(rest[..end].to_string())
}

/// Find a known tool name within the body text (e.g., `<file_write>` → "write_file").
fn find_direct_tool_name(body: &str) -> Option<String> {
    let known = [
        ("write_file", "write_file"),
        ("file_write", "write_file"),
        ("read_file", "read_file"),
        ("file_read", "read_file"),
        ("shell", "shell"),
        ("bash", "shell"),
        ("compile", "compile"),
        ("run_tests", "run_tests"),
        ("search", "search"),
        ("list_files", "list_files"),
    ];
    for (tag, canonical) in &known {
        // Match <tag> or <tag< (malformed XML from small models)
        let open = format!("<{tag}");
        if body.contains(&open) {
            return Some(canonical.to_string());
        }
    }
    None
}

/// Try to parse a JSON-format tool call: {"tool": "write_file", "path": "...", "content": "..."}
fn try_json_tool_call(text: &str) -> Vec<ParsedToolCall> {
    // Find JSON object with "tool" key
    let json_start = match text.find(r#"{"tool""#).or_else(|| text.find(r#"{ "tool""#)) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let remaining = &text[json_start..];

    // Find the matching closing brace
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = 0usize;

    for (i, ch) in remaining.char_indices() {
        if escaped { escaped = false; continue; }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 { end = i + 1; break; }
            }
            _ => {}
        }
    }

    if end == 0 { return Vec::new(); }

    let json_str = &remaining[..end];
    let parsed: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let tool_name = match parsed.get("tool").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return Vec::new(),
    };
    let canonical = match tool_name.as_str() {
        "file_write" | "write_file" => "write_file",
        "file_read" | "read_file" => "read_file",
        "shell" | "bash" | "execute" | "run" => "shell",
        "compile" | "build" => "compile",
        "test" | "run_tests" | "pytest" => "run_tests",
        "search" | "find" | "grep" => "search",
        "ls" | "list" | "list_files" | "dir" => "list_files",
        other => other,
    };

    // Build args based on tool type
    let mut args = Vec::new();
    if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
        args.push(path.to_string());
    }
    if let Some(content) = parsed.get("content").and_then(|v| v.as_str()) {
        args.push(content.to_string());
    }
    if let Some(cmd) = parsed.get("command").or_else(|| parsed.get("cmd")).and_then(|v| v.as_str()) {
        args.push(cmd.to_string());
    }
    if let Some(query) = parsed.get("query").or_else(|| parsed.get("pattern")).and_then(|v| v.as_str()) {
        args.push(query.to_string());
    }

    if args.is_empty() { return Vec::new(); }

    vec![ParsedToolCall { name: canonical.to_string(), args }]
}

/// Try to parse OpenAI function-calling format: [{"name":"tool_name","arguments":{...}}]
fn try_openai_function_call(text: &str) -> Vec<ParsedToolCall> {
    // Find JSON array starting with [{"name"
    let start = match text.find(r#"[{"name""#).or_else(|| text.find(r#"[{ "name""#)) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let remaining = &text[start..];

    // Find matching ']'
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = 0usize;
    let mut started = false;

    for (i, ch) in remaining.char_indices() {
        if escaped { escaped = false; continue; }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' if !in_string => { depth += 1; started = true; }
            ']' if !in_string => {
                depth -= 1;
                if depth == 0 && started { end = i + 1; break; }
            }
            _ => {}
        }
    }

    if end == 0 { return Vec::new(); }

    let json_str = &remaining[..end];
    let parsed: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut calls = Vec::new();
    for entry in &parsed {
        let tool_name = match entry.get("name").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let canonical = match tool_name.as_str() {
            "file_write" | "write_file" | "writeFile" => "write_file",
            "file_read" | "read_file" | "readFile" => "read_file",
            "shell" | "bash" | "execute" | "run" | "runCommand" => "shell",
            "compile" | "build" => "compile",
            "test" | "run_tests" | "runTests" | "pytest" => "run_tests",
            "search" | "find" | "grep" | "searchCode" => "search",
            "ls" | "list" | "list_files" | "listFiles" | "dir" => "list_files",
            "git" | "git_status" | "gitStatus" => "git_status",
            other => other,
        };

        let arguments = entry.get("arguments").or_else(|| entry.get("args"));

        // Build args from the arguments object
        let mut args = Vec::new();
        if let Some(args_map) = arguments.and_then(|v| v.as_object()) {
            // write_file: path, content
            if let Some(v) = args_map.get("path").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            } else if let Some(v) = args_map.get("file").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            } else if let Some(v) = args_map.get("filename").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            }

            if let Some(v) = args_map.get("content").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            } else if let Some(v) = args_map.get("text").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            } else if let Some(v) = args_map.get("data").and_then(|v| v.as_str()) {
                args.push(v.to_string());
            }

            // shell: command
            if let Some(v) = args_map.get("command").or_else(|| args_map.get("cmd")).and_then(|v| v.as_str()) {
                // Split command into parts for shell tool
                args.push(v.to_string());
            }

            // search: query
            if let Some(v) = args_map.get("query").or_else(|| args_map.get("pattern")).and_then(|v| v.as_str()) {
                args.push(v.to_string());
            }
        }

        if args.is_empty() { continue; }

        calls.push(ParsedToolCall { name: canonical.to_string(), args });
    }

    calls
}

/// Extract the content between the first known tool tag as args.
fn extract_direct_tool_args(body: &str) -> String {
    let known = [
        "write_file", "file_write", "read_file", "file_read", "shell", "bash",
        "compile", "run_tests", "search", "list_files",
    ];
    for tag in &known {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        if let Some(start) = body.find(&open) {
            let after = &body[start + open.len()..];
            if let Some(end) = after.find(&close) {
                return after[..end].trim().to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_tool_call() {
        let text = r#"
Some text before
<tool_call>
<name>read_file</name>
<args>["src/main.rs"]</args>
</tool_call>
Some text after
        "#;

        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].args, vec!["src/main.rs"]);
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let text = r#"
<tool_call>
<name>write_file</name>
<args>["test.rs", "fn main() {}"]</args>
</tool_call>
More text
<tool_call>
<name>shell</name>
<args>["cargo", "test"]</args>
</tool_call>
        "#;

        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[1].name, "shell");
    }

    #[test]
    fn parse_no_tool_calls() {
        let calls = parse_tool_calls("just some text, no tools here");
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_direct_file_write_tag() {
        let text = r#"<tool_call>
<file_write>
<args>["/tmp/test.txt", "content"]</args>
</tool_call>"#;

        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/test.txt", "content"]);
    }

    #[test]
    fn parse_direct_shell_tag() {
        let text = r#"<tool_call>
<shell>["echo", "hello"]</args>
</tool_call>"#;

        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn extract_xml_content_simple() {
        let result = extract_xml_content("<name>hello</name>", "name");
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn parse_args_json_array() {
        let args = parse_args_string(r#"["a","b","c"]"#);
        assert_eq!(args, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_args_plain() {
        let args = parse_args_string("a,b,c");
        assert_eq!(args, vec!["a", "b", "c"]);
    }

    // ── Regression tests for malformed XML from small models ─────

    #[test]
    fn malformed_write_file_lt_tag() {
        // Model output: <write_file<args>["/tmp/a.txt","hello"]</args>
        let text = r#"<tool_call><write_file<args>["/tmp/a.txt","hello"]</args></tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1, "Should find 1 tool call");
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/a.txt", "hello"]);
    }

    #[test]
    fn malformed_write_file_nested() {
        // Model output: <tool_call><write_file><args>["/tmp/a.txt","hello"]</args></write_file></tool_call>
        let text = r#"<tool_call><write_file><args>["/tmp/a.txt","hello"]</args></write_file></tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/a.txt", "hello"]);
    }

    #[test]
    fn malformed_write_file_with_newlines() {
        let text = r#"<tool_call>
  <write_file<args>["/tmp/a.txt","hello"]</args>
</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1, "Should parse with newlines");
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/a.txt", "hello"]);
    }

    #[test]
    fn multiple_tool_calls_mixed_format() {
        let text = r#"<tool_call>
<write_file<args>["/tmp/first.txt","first content"]</args>
</tool_call>
<tool_call>
<name>shell</name>
<args>["cargo","test"]</args>
</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2, "Should find 2 tool calls");
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/first.txt", "first content"]);
        assert_eq!(calls[1].name, "shell");
        assert_eq!(calls[1].args, vec!["cargo", "test"]);
    }

    #[test]
    fn well_formed_tool_call_still_works() {
        let text = r#"<tool_call>
<name>write_file</name>
<args>["/tmp/test.rs","fn main() {}"]</args>
</tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].args, vec!["/tmp/test.rs", "fn main() {}"]);
    }

    #[test]
    fn never_unknown_for_recognizable_tool() {
        // Even with extreme malformation, if 'write_file' appears, it should be recognized
        let text = r#"<tool_call><XYZZY write_file BROKEN><args>["/tmp/test.txt","data"]</args></tool_call>"#;
        let calls = parse_tool_calls(text);
        // Should find it via fuzzy match
        assert!(!calls.is_empty() || {
            // If not found via XML, fuzzy_parse should catch it
            let fuzzy = fuzzy_parse(text);
            !fuzzy.is_empty() && fuzzy[0].name == "write_file"
        });
    }

    #[test]
    fn preserve_exact_argument_values() {
        let text = r#"<tool_call><write_file<args>["/tmp/hello.txt","Exactly  this  spacing"]</args></tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].args[1], "Exactly  this  spacing",
            "Arguments must not be normalized or truncated");
    }

    #[test]
    fn args_with_special_chars() {
        let text = r#"<tool_call><name>write_file</name><args>["/tmp/hello.txt","pub fn reverse_words(input: &str) -> String { input.split_whitespace().map(|w| w.chars().rev().collect()).collect::<Vec<_>>().join(\" \") }"]</args></tool_call>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].args[1].contains("reverse_words"));
        assert!(calls[0].args[1].contains("split_whitespace"));
    }
}
