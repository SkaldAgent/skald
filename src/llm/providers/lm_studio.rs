use async_trait::async_trait;
use anyhow::{anyhow, Result};

use super::{ModelType, ProviderCaps, RemoteModelInfo};

pub struct LmStudioProvider {
    base_url: String,
    http:     reqwest::Client,
}

impl LmStudioProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), http: reqwest::Client::new() }
    }
}

#[async_trait]
impl ProviderCaps for LmStudioProvider {
    fn supported_types(&self) -> &'static [ModelType] {
        &[ModelType::Llm]
    }

    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));

        let resp: serde_json::Value = self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("LM Studio request failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("LM Studio response parse failed: {e}"))?;

        let models = resp["data"]
            .as_array()
            .ok_or_else(|| anyhow!("unexpected LM Studio response shape"))?
            .iter()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
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
}
