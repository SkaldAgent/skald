use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

/// Implemented by any provider that can convert text to audio bytes.
/// Returns raw audio bytes (MP3 expected unless the provider states otherwise).
#[async_trait]
pub trait TextToSpeech: Send + Sync {
    /// A stable, unique identifier for this provider (e.g. `"openai_tts_alloy"`).
    fn id(&self) -> &str;
    /// Human-readable display name.
    fn name(&self) -> &str;
    /// Human-readable description (voice style, language, ideal use cases).
    fn description(&self) -> Option<&str> { None }
    /// Default synthesis instructions: voice style, tone, speed, and any
    /// provider-specific text markup syntax (e.g. emotion tags).
    /// Surfaced to the LLM via `TtsModelInfo` so it knows how to format input text.
    /// Individual call-time instructions passed to `synthesize` take precedence.
    fn instructions(&self) -> Option<&str> { None }
    /// Synthesise `text` to audio bytes.
    /// `instructions` overrides the provider's default instructions for this call only.
    async fn synthesize(&self, text: &str, instructions: Option<&str>) -> Result<Vec<u8>>;
}

/// Resolves the currently active [`TextToSpeech`] provider.
///
/// Implemented by `TtsManager` in the main crate. Plugins store
/// `Arc<dyn TtsProvider>` to resolve the active synthesiser per-call
/// without holding a reference to `AppState`.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    async fn get(&self) -> Option<Arc<dyn TextToSpeech>>;
}

/// Write-side of the TTS manager: register and remove ephemeral providers.
///
/// Implemented by `TtsManager`. Plugins that supply their own TTS engine
/// (e.g. a local Kokoro or Piper plugin) use this to register at start
/// and unregister at stop.
#[async_trait]
pub trait TtsRegistry: Send + Sync {
    async fn register(&self, provider: Arc<dyn TextToSpeech>);
    async fn unregister(&self, id: &str);
}
