use anyhow::Result;
use serde_json::Value;

// ── ToolDescriptionLength ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDescriptionLength {
    Short,
    Full,
}

// ── ToolCategory ──────────────────────────────────────────────────────────────

/// Logical grouping for a tool.
///
/// Used for access-control filtering and for display/audit purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    /// Read or write files on disk.
    Filesystem,
    /// Run shell commands or restart the process.
    Shell,
    /// Invoke sub-agents via call_agent.
    Subagent,
    /// Read-only discovery of system state.
    Introspection,
    /// Mutate system configuration.
    Config,
}

// ── Tool trait ────────────────────────────────────────────────────────────────

/// A single LLM-callable tool.
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;

    /// Human-readable label for this tool invocation shown in UI / notifications.
    fn describe(&self, _args: &Value, _length: ToolDescriptionLength) -> String {
        self.name().to_string()
    }

    /// JSON Schema for the `parameters` field in the OpenAI function definition.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool synchronously and return a plain-text result (or error string).
    /// Tools that require async I/O should override `execute_async` instead.
    fn execute(&self, _args: Value) -> Result<String> {
        Err(anyhow::anyhow!("tool '{}': sync execute not implemented — use execute_async", self.name()))
    }

    /// Execute the tool asynchronously. The default wraps `execute`; async tools
    /// (e.g. image generation) override this directly to avoid `block_in_place`.
    fn execute_async<'a>(&'a self, args: Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move { self.execute(args) })
    }

    /// Logical category of this tool.
    fn category(&self) -> ToolCategory;

    /// If true, this tool is only included in the tool list for sub-agents (depth > 0).
    fn sub_agents_only(&self) -> bool { false }

    /// If true, this tool is only included in the tool list for the root agent (depth == 0).
    fn root_agent_only(&self) -> bool { false }

    /// Full OpenAI-format tool definition ready to be sent to the LLM.
    fn openai_definition(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name":        self.name(),
                "description": self.description(),
                "parameters":  self.parameters_schema(),
            }
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Truncates a label to `max` chars, appending `…` if cut.
pub fn truncate_label(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut = max.saturating_sub(1);
    let mut end = cut;
    while !s.is_char_boundary(end) { end -= 1; }
    format!("{}…", &s[..end])
}
