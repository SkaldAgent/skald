use async_trait::async_trait;
use anyhow::Result;

use super::{ModelType, ProviderCaps, RemoteModelInfo};

pub struct OpenAiProvider;

#[async_trait]
impl ProviderCaps for OpenAiProvider {
    fn supported_types(&self) -> &'static [ModelType] {
        &[ModelType::Llm, ModelType::Transcribe]
    }

    async fn list_models(&self) -> Result<Option<Vec<RemoteModelInfo>>> {
        Ok(None)
    }
}
