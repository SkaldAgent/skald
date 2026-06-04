pub mod anthropic;
pub mod lm_studio;
pub mod ollama;
pub mod openai;

use async_trait::async_trait;
use serde_json::Value;

/// A single message in a conversation.
#[derive(Debug, Clone)]
pub struct Message {
    pub role:    Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
}

/// Options for a single chat completion request.
#[derive(Debug, Clone)]
pub struct ChatOptions {
    pub model:       String,
    pub max_tokens:  Option<u32>,
    pub temperature: Option<f32>,
    /// Session/stack IDs for request logging. Set by the LLM loop; ignored by
    /// providers — only the logging wrapper reads them.
    pub session_id:  Option<i64>,
    pub stack_id:    Option<i64>,
}

/// Raw HTTP metadata captured during a provider call.
/// Sensitive header values (api_key) are redacted before storage.
#[derive(Debug, Default)]
pub struct LlmRawMeta {
    /// HTTP request headers sent to the provider (api-key redacted).
    pub request_headers:  Option<Value>,
    /// Full HTTP request body sent to the provider (provider-specific format).
    pub request_body:     Option<Value>,
    /// HTTP response headers received from the provider.
    pub response_headers: Option<Value>,
    /// Full HTTP response body (raw JSON before parsing).
    pub response_body:    Option<Value>,
}

// ── Utility ───────────────────────────────────────────────────────────────────

/// Converts a reqwest `HeaderMap` into a `serde_json::Value` object.
pub fn headers_to_json(headers: &reqwest::header::HeaderMap) -> Value {
    let map: serde_json::Map<String, Value> = headers
        .iter()
        .map(|(k, v)| (
            k.as_str().to_string(),
            v.to_str().unwrap_or("<binary>").into(),
        ))
        .collect();
    Value::Object(map)
}

/// Returns a redacted preview of an API key: first 7 chars + "***".
pub fn redact_key(key: &str) -> String {
    if key.len() > 7 {
        format!("{}***", &key[..7])
    } else {
        "***".to_string()
    }
}

/// The response from a chat completion (text only).
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content:           String,
    pub input_tokens:      Option<u32>,
    pub output_tokens:     Option<u32>,
    /// True when the model stopped due to hitting the token limit (finish_reason="length").
    pub truncated:         bool,
    /// Chain-of-thought produced by reasoning models (e.g. DeepSeek thinking mode).
    /// Must be echoed back in the assistant message on subsequent turns.
    pub reasoning_content: Option<String>,
}

/// A single tool call requested by the LLM.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// String ID assigned by the LLM (e.g. "call_abc123"). Used to match results.
    pub id:        String,
    pub name:      String,
    pub arguments: Value,
}

/// Result of one LLM turn when tools are available.
#[derive(Debug)]
pub enum LlmTurn {
    /// The LLM produced a final text answer — conversation turn is complete.
    Message(ChatResponse),
    /// The LLM wants to invoke tools before answering.
    ToolCalls {
        /// Optional text the LLM produced alongside the tool calls (often empty).
        content:           String,
        calls:             Vec<ToolCall>,
        input_tokens:      Option<u32>,
        output_tokens:     Option<u32>,
        reasoning_content: Option<String>,
    },
}

/// Stateless LLM client. Implementations hold only connection config (base URL,
/// API key). No memory, no database, no session state.
#[async_trait]
pub trait ChatbotClient: Send + Sync {
    async fn chat(
        &self,
        messages: &[Message],
        options:  &ChatOptions,
    ) -> anyhow::Result<ChatResponse>;

    /// Chat with tool support. `messages` is a raw OpenAI-format array (role +
    /// content, plus assistant/tool entries for prior tool turns).
    /// `tools` is the list of OpenAI-format function definitions.
    ///
    /// Default implementation ignores tools and falls back to `chat()`.
    async fn chat_with_tools(
        &self,
        messages: &[Value],
        tools:    &[Value],
        options:  &ChatOptions,
    ) -> anyhow::Result<LlmTurn> {
        let simple: Vec<Message> = messages
            .iter()
            .filter_map(|m| {
                let role    = m["role"].as_str()?;
                let content = m["content"].as_str().unwrap_or("").to_string();
                match role {
                    "system"    => Some(Message::system(content)),
                    "user"      => Some(Message::user(content)),
                    "assistant" => Some(Message::assistant(content)),
                    _           => None,
                }
            })
            .collect();
        let _ = tools;
        let resp = self.chat(&simple, options).await?;
        Ok(LlmTurn::Message(resp))
    }

    /// Like `chat_with_tools` but also returns raw HTTP metadata (request/response
    /// headers and bodies) for the logging wrapper to persist.
    ///
    /// Providers that make real HTTP calls should override this to capture wire-level
    /// data. The default calls `chat_with_tools` and returns `None` metadata, so
    /// providers that don't override still work — they just won't log HTTP headers.
    async fn chat_with_tools_raw(
        &self,
        messages: &[Value],
        tools:    &[Value],
        options:  &ChatOptions,
    ) -> anyhow::Result<(LlmTurn, Option<LlmRawMeta>)> {
        self.chat_with_tools(messages, tools, options).await.map(|t| (t, None))
    }
}
