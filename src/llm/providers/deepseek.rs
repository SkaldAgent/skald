use async_trait::async_trait;
use anyhow::{anyhow, Result};

use super::{ModelType, ProviderCaps, RemoteModelInfo};

pub struct DeepSeekProvider {
    api_key: String,
    http:    reqwest::Client,
}

impl DeepSeekProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self { api_key: api_key.into(), http: reqwest::Client::new() }
    }

    fn known_context_length(model_id: &str) -> Option<u64> {
        let id = model_id.to_lowercase();
        if id.contains("coder") {
            Some(16384)
        } else if id.contains("reasoner") {
            Some(65536)
        } else if id.starts_with("deepseek-v4") {
            Some(1_048_576)
        } else if id.starts_with("deepseek-chat") || id.starts_with("deepseek-v3") {
            Some(65536)
        } else {
            None
        }
    }

    fn known_max_output(model_id: &str) -> Option<u64> {
        let id = model_id.to_lowercase();
        if id.starts_with("deepseek-v4") {
            Some(393_216)
        } else {
            None
        }
    }

    fn known_capabilities(model_id: &str) -> Vec<String> {
        let mut caps = vec!["function_calling".to_string()];
        let id = model_id.to_lowercase();
        if id.contains("reasoner") {
            caps.push("reasoning".to_string());
        }
        caps
    }
}

#[async_trait]
impl ProviderCaps for DeepSeekProvider {
    fn supported_types(&self) -> &'static [ModelType] {
        &[ModelType::Llm]
    }

    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>> {
        let resp: serde_json::Value = self.http
            .get("https://api.deepseek.com/models")
            .bearer_auth(&self.api_key)
            .send()
            .await
            .map_err(|e| anyhow!("DeepSeek request failed: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("DeepSeek error response: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("DeepSeek response parse failed: {e}"))?;

        let models = resp["data"]
            .as_array()
            .ok_or_else(|| anyhow!("unexpected DeepSeek response shape"))?
            .iter()
            .filter_map(|m| {
                let id   = m["id"].as_str()?.to_string();
                let name = m["id"].as_str()?.to_string();
                let context_length = Self::known_context_length(&id).or_else(|| m["context_length"].as_u64());
                let capabilities   = Self::known_capabilities(&id);
                let max_output     = Self::known_max_output(&id);
                Some(RemoteModelInfo {
                    id,
                    name,
                    context_length,
                    max_completion_tokens:    max_output,
                    knowledge_cutoff:         None,
                    capabilities,
                    price_input_per_million:  None,
                    price_output_per_million: None,
                })
            })
            .collect();

        Ok(Some(models))
    }
}
