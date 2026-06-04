pub(crate) mod db;
pub mod manager;
pub mod providers;

use std::sync::Arc;

use crate::chatbot::ChatbotClient;
use crate::config::{LlmProvider, LlmStrength};
use providers::ModelType;

pub use manager::{LlmManager, sort_models_for_agent};

/// A resolved, ready-to-use LLM client with its associated metadata.
#[derive(Clone)]
pub struct LlmEntry {
    pub client:          Arc<dyn ChatbotClient>,
    pub model:           String,
    pub model_db_id:     i64,
    pub strength:        Option<LlmStrength>,
    pub scope:           Vec<String>,
    pub extra_params:    Option<serde_json::Value>,
    /// Max input context window in tokens, if known.
    pub context_length:  Option<i64>,
    /// When true, prompt-caching hints are injected into requests:
    /// - System messages are split into a cached static block and an uncached
    ///   dynamic block (date/time, scratchpad).
    /// - The last tool definition is tagged with `cache_control: ephemeral`.
    /// - The `anthropic-beta: prompt-caching-2024-07-31` header is sent.
    /// Currently enabled for OpenRouter (Anthropic models) only.
    pub prompt_cache:    bool,
}

// ── Provider ──────────────────────────────────────────────────────────────────

/// Full provider record (includes secrets — only expose over trusted local connections).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmProviderRecord {
    pub id:          i64,
    pub name:        String,
    #[serde(rename = "type")]
    pub provider:    LlmProvider,
    pub api_key:     Option<String>,
    /// Only used by ollama and lm_studio.
    pub base_url:    Option<String>,
    pub description: Option<String>,
}

/// Public provider metadata (no api_key).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LlmProviderInfo {
    pub id:              i64,
    pub name:            String,
    #[serde(rename = "type")]
    pub provider:        LlmProvider,
    pub base_url:        Option<String>,
    pub description:     Option<String>,
    /// Model types this provider supports (hardcoded per provider implementation).
    pub supported_types: Vec<ModelType>,
}

// ── Model ─────────────────────────────────────────────────────────────────────

/// Full model record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmModelRecord {
    pub id:               i64,
    pub provider_id:      i64,
    pub model_id:         String,
    /// Display alias. If empty/null at the DB level we fall back to model_id.
    pub name:             String,
    pub strength:         Option<LlmStrength>,
    pub scope:            Vec<String>,
    pub is_default:       bool,
    pub priority:         i32,
    pub extra_params:     Option<serde_json::Value>,
    /// Max input context window in tokens, if known (from provider catalog or manual).
    pub context_length:   Option<i64>,
    /// Max output tokens, if known (from provider catalog or manual).
    pub max_output_tokens: Option<i64>,
    /// Date string like "2024-09-01", if known.
    pub knowledge_cutoff: Option<String>,
    /// Capabilities (e.g. "function_calling", "vision", "streaming", …).
    pub capabilities:     Vec<String>,
}

/// Public model metadata for API responses (includes provider name for convenience).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LlmModelInfo {
    pub id:                       i64,
    pub provider_id:              i64,
    pub provider_name:            String,
    pub model_id:                 String,
    pub name:                     String,
    pub strength:                 Option<LlmStrength>,
    pub scope:                    Vec<String>,
    pub is_default:               bool,
    pub priority:                 i32,
    pub extra_params:             Option<serde_json::Value>,
    pub context_length:           Option<i64>,
    pub max_output_tokens:        Option<i64>,
    pub knowledge_cutoff:         Option<String>,
    pub capabilities:             Vec<String>,
    pub status:                   ClientStatus,
    pub last_error:               Option<String>,
    /// Input (prompt) price per million tokens (USD) from the provider catalog cache.
    pub price_input_per_million:  Option<f64>,
    /// Output (completion) price per million tokens (USD) from the provider catalog cache.
    pub price_output_per_million: Option<f64>,
}

// ── Health ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientStatus {
    Healthy,
    Degraded,
    Down,
}
