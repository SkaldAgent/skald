use async_trait::async_trait;
use anyhow::{anyhow, Result};

use super::{ModelType, ProviderCaps, RemoteModelInfo};

pub struct AnthropicProvider {
    api_key: String,
    http:    reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { api_key: api_key.into(), http: reqwest::Client::new() }
    }
}

#[async_trait]
impl ProviderCaps for AnthropicProvider {
    fn supported_types(&self) -> &'static [ModelType] {
        &[ModelType::Llm]
    }

    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>> {
        Ok(None)
    }

    async fn model_info(&self, model_id: &str) -> Result<Option<RemoteModelInfo>> {
        let url = format!("https://api.anthropic.com/v1/models/{model_id}");
        let resp: serde_json::Value = self.http
            .get(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| anyhow!("Anthropic model_info request failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Anthropic model_info response parse failed: {e}"))?;

        let id = resp["id"].as_str().ok_or_else(|| anyhow!("missing 'id' in Anthropic response"))?.to_string();
        let name = resp["display_name"].as_str().unwrap_or(&id).to_string();
        let context_length = resp["context_window"].as_u64();
        let max_completion_tokens = resp["max_output_tokens"].as_u64();

        Ok(Some(RemoteModelInfo {
            id,
            name,
            context_length,
            max_completion_tokens,
            knowledge_cutoff: None,
            capabilities: vec![],
            price_input_per_million: None,
            price_output_per_million: None,
        }))
    }
}