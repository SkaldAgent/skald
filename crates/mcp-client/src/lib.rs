pub mod config;
pub mod http_server;
pub mod server;

use async_trait::async_trait;
use serde_json::{Value, json};

pub use config::{McpServerConfig, McpTransport};
pub use server::{
    ElicitationAction, ElicitationHandler, ElicitationReply, ElicitationRequest, McpNotification,
};

/// MCP protocol version this client advertises in `initialize` and (over HTTP)
/// echoes in the `MCP-Protocol-Version` header on every post-initialize request.
/// Shared by both transports so they can never drift apart. The host tolerates a
/// server negotiating a different (older) version — see the HTTP transport, which
/// captures the server's reply and warns rather than disconnecting.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// Safety cap on `tools/list` pagination: stop following `nextCursor` after this
/// many pages so a buggy or hostile server that never clears the cursor can't
/// loop the client forever.
pub(crate) const MAX_TOOL_PAGES: usize = 50;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server_name:   String,
    pub name:          String,
    pub description:   String,
    pub input_schema:  Value,
    /// Optional human-readable title (MCP 2025-06-18+).
    pub title:         Option<String>,
    /// Optional JSON Schema describing the tool's structured output
    /// (`outputSchema`, MCP 2025-06-18+). Captured for future validation and to
    /// know a tool can return `structuredContent`.
    pub output_schema: Option<Value>,
    /// Optional tool annotations (`readOnlyHint`, `destructiveHint`, …), treated
    /// as untrusted hints per the spec.
    pub annotations:   Option<Value>,
}

impl McpTool {
    /// Builds an [`McpTool`] from one entry of a `tools/list` `tools[]` array.
    /// Shared by both transports so the field mapping (incl. the 2025-06-18+
    /// `title`/`outputSchema`/`annotations`) stays in one place.
    pub fn from_json(server_name: &str, t: &Value) -> McpTool {
        McpTool {
            server_name:   server_name.to_string(),
            name:          t["name"].as_str().unwrap_or("").to_string(),
            description:   t["description"].as_str().unwrap_or("").to_string(),
            input_schema:  t.get("inputSchema").cloned().unwrap_or_else(|| json!({
                "type": "object", "properties": {},
            })),
            title:         t.get("title").and_then(Value::as_str).map(str::to_string),
            output_schema: t.get("outputSchema").cloned(),
            annotations:   t.get("annotations").cloned(),
        }
    }

    pub fn tool_id(&self) -> String {
        format!("mcp__{}__{}", self.server_name, self.name)
    }

    pub fn to_openai_definition(&self) -> Value {
        let params = if self.input_schema.is_object() {
            self.input_schema.clone()
        } else {
            json!({ "type": "object", "properties": {} })
        };
        json!({
            "type": "function",
            "function": {
                "name":        self.tool_id(),
                "description": format!("[{}] {}", self.server_name, self.description),
                "parameters":  params,
            }
        })
    }
}

/// Status of a configured MCP server.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum McpServerStatus {
    Running { tools: Vec<String> },
    Error   { message: String },
    Disabled,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServerInfo {
    pub name:      String,
    pub transport: String,
    pub description:   Option<String>,
    pub friendly_name: Option<String>,
    #[serde(flatten)]
    pub status:    McpServerStatus,
}

// ── Transport trait ───────────────────────────────────────────────────────────

/// Typed result of an MCP `tools/call`. Mirrors the host's tool-result shape
/// without coupling this crate to it; the host (`McpManager`) maps it
/// (text → `Text`, structured → `Json`). Per the MCP spec, `structuredContent`
/// is canonical when present (servers SHOULD also mirror it in a `TextContent`
/// block for backwards compatibility); we prefer it and fall back to the joined
/// `text` items, which fixes the silent empty-result case for servers that omit
/// the text mirror.
#[derive(Debug, Clone)]
pub enum McpCallResult {
    /// Joined `text` content items.
    Text(String),
    /// Canonical structured payload from `structuredContent`.
    Json(Value),
}

#[async_trait]
pub trait McpServerClient: Send + Sync {
    fn tools(&self) -> &[McpTool];
    async fn call_tool(&self, name: &str, args: Value) -> anyhow::Result<McpCallResult>;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parses `mcp__<server>__<tool>` → `(server, tool)`.
pub fn parse_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    let sep = rest.find("__")?;
    Some((&rest[..sep], &rest[sep + 2..]))
}

/// Extracts text content from an MCP tool result value.
pub(crate) fn extract_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Builds the typed result of an MCP `tools/call`. Prefers `structuredContent`
/// (canonical per spec when present) and falls back to the joined `text` items —
/// which also fixes the silent empty-result case for servers that return only
/// `structuredContent` without the recommended text mirror.
pub(crate) fn extract_call_result(result: &Value) -> McpCallResult {
    if let Some(sc) = result.get("structuredContent") {
        if !sc.is_null() {
            return McpCallResult::Json(sc.clone());
        }
    }
    McpCallResult::Text(extract_text(result))
}

/// Interpolates `${VAR}` references in a string from the process environment.
pub(crate) fn interpolate_env(s: &str) -> String {
    let mut result = s.to_string();
    loop {
        let Some(start) = result.find("${") else { break };
        let Some(rel_end) = result[start..].find('}') else { break };
        let var_name = result[start + 2..start + rel_end].to_string();
        let value = std::env::var(&var_name).unwrap_or_else(|_| {
            tracing::warn!("MCP env var ${{{var_name}}} not set");
            String::new()
        });
        result = format!("{}{}{}", &result[..start], value, &result[start + rel_end + 1..]);
    }
    result
}
