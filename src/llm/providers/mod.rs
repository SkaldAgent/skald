pub mod anthropic;
pub mod deepseek;
pub mod lm_studio;
pub mod ollama;
pub mod openai;
pub mod openrouter;

// Re-export so existing code that uses `providers::ServiceType` / `providers::RemoteLlmModelInfo` keeps working.
pub use crate::provider::ServiceType;
pub use core_api::provider::RemoteLlmModelInfo;
