use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use crate::chatbot::anthropic::AnthropicClient;
use crate::llm::{LlmModelRecord, LlmProviderRecord};
use crate::llm::providers::RemoteLlmModelInfo;
use crate::provider::{ApiProvider, BuiltLlmClient, ProviderField, ProviderUiMeta, ServiceType};

pub struct AnthropicProvider {
    http: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self { http: reqwest::Client::new() }
    }
}

#[async_trait::async_trait]
impl ApiProvider for AnthropicProvider {
    fn type_id(&self) -> &'static str { "anthropic" }
    fn display_name(&self) -> &'static str { "Anthropic" }
    fn supported_types(&self) -> &'static [ServiceType] {
        &[ServiceType::Llm]
    }

    async fn list_llm_models(&self, _record: &LlmProviderRecord) -> Result<Option<Vec<RemoteLlmModelInfo>>> {
        Ok(None)
    }

    async fn llm_model_info(&self, record: &LlmProviderRecord, model_id: &str) -> Result<Option<RemoteLlmModelInfo>> {
        let api_key = record.api_key.as_deref()
            .ok_or_else(|| anyhow!("provider '{}': api_key required for anthropic model_info", record.name))?;

        let url = format!("https://api.anthropic.com/v1/models/{model_id}");
        let resp: serde_json::Value = self.http
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| anyhow!("Anthropic model_info request failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Anthropic model_info response parse failed: {e}"))?;

        let id   = resp["id"].as_str().ok_or_else(|| anyhow!("missing 'id' in Anthropic response"))?.to_string();
        let name = resp["display_name"].as_str().unwrap_or(&id).to_string();

        Ok(Some(RemoteLlmModelInfo {
            id,
            name,
            context_length:           resp["context_window"].as_u64(),
            max_completion_tokens:    resp["max_output_tokens"].as_u64(),
            knowledge_cutoff:         None,
            capabilities:             vec![],
            vision:                   None,
            price_input_per_million:  None,
            price_output_per_million: None,
        }))
    }

    fn build_llm(&self, record: &LlmProviderRecord, _model: &LlmModelRecord) -> Option<Result<BuiltLlmClient>> {
        Some((|| {
            let key = record.api_key.as_deref()
                .with_context(|| format!("provider '{}': api_key required for anthropic", record.name))?;
            Ok(BuiltLlmClient {
                client: Arc::new(AnthropicClient::new(key)),
                prompt_cache: false,
            })
        })())
    }

    fn ui_meta(&self) -> ProviderUiMeta {
        ProviderUiMeta {
            type_id:      "anthropic",
            display_name: "Anthropic",
            description:  None,
            color:        "#d4a574",
            icon:         "bi-chat-square-dots",
            fields: &[
                ProviderField { key: "api_key", label: "API Key", required: true, secret: true },
            ],
        }
    }
}
