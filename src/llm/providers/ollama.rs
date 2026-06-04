use async_trait::async_trait;
use anyhow::{anyhow, Result};

use super::{ModelType, ProviderCaps, RemoteModelInfo};

pub struct OllamaProvider {
    base_url: String,
    http:     reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), http: reqwest::Client::new() }
    }
}

impl OllamaProvider {
    fn parse_model_info(&self, show: &serde_json::Value, model_id: &str) -> RemoteModelInfo {
        let context_length = show["model_info"]["llm.context_length"]
            .as_u64()
            .or_else(|| {
                show["model_info"]["llm.context_length"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
            });

        RemoteModelInfo {
            name:                     model_id.to_string(),
            id:                       model_id.to_string(),
            context_length,
            max_completion_tokens:    None,
            knowledge_cutoff:         None,
            capabilities:             vec![],
            price_input_per_million:  None,
            price_output_per_million: None,
        }
    }
}

#[async_trait]
impl ProviderCaps for OllamaProvider {
    fn supported_types(&self) -> &'static [ModelType] {
        &[ModelType::Llm]
    }

    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>> {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));

        let resp: serde_json::Value = self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Ollama request failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Ollama response parse failed: {e}"))?;

        let models = resp["models"]
            .as_array()
            .ok_or_else(|| anyhow!("unexpected Ollama response shape"))?
            .iter()
            .filter_map(|m| {
                let id = m["name"].as_str()?.to_string();
                Some(RemoteModelInfo {
                    name:                     id.clone(),
                    id,
                    context_length:           None,
                    max_completion_tokens:    None,
                    knowledge_cutoff:         None,
                    capabilities:             vec![],
                    price_input_per_million:  None,
                    price_output_per_million: None,
                })
            })
            .collect();

        Ok(Some(models))
    }

    async fn model_info(&self, model_id: &str) -> Result<Option<RemoteModelInfo>> {
        let url = format!("{}/api/show", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({ "name": model_id });

        let resp: serde_json::Value = self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow!("Ollama model_info request failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Ollama model_info response parse failed: {e}"))?;

        Ok(Some(self.parse_model_info(&resp, model_id)))
    }
}
