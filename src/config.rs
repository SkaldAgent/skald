use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

pub use core_api::provider::LlmStrength;
pub use crate::core::config::{
    DbConfig, LlmConfig, TicConfig, CronConfig,
    CompactionConfig, DatetimeConfig, LlmRequestsLogConfig,
};

const DEFAULT_CONFIG: &str = "default.config.yaml";
const CONFIG: &str = "config.yml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server:   ServerConfig,
    pub web:      WebConfig,
    pub db:       DbConfig,
    pub llm:      LlmConfig,
    #[serde(default)]
    pub tic:      TicConfig,
    #[serde(default)]
    pub cron:     CronConfig,
    /// Global IANA timezone name (e.g. `"Europe/Rome"`).
    /// Applied to: cron expression evaluation, datetime injected into the LLM context.
    /// When omitted, the server's local system timezone is used everywhere.
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct WebConfig {
    pub static_dir: String,
}

impl Config {
    pub fn into_split(self) -> (crate::core::config::CoreConfig, crate::frontend::config::FrontendConfig) {
        let tz = self.timezone.clone();
        (
            crate::core::config::CoreConfig {
                db:       self.db,
                llm:      self.llm,
                tic:      self.tic,
                cron:     self.cron,
                timezone: self.timezone,
            },
            crate::frontend::config::FrontendConfig {
                server:   self.server,
                web:      self.web,
                timezone: tz,
            },
        )
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path  = Path::new(CONFIG);
        let default_path = Path::new(DEFAULT_CONFIG);

        if !config_path.exists() {
            std::fs::copy(default_path, config_path)
                .with_context(|| format!("Failed to copy {DEFAULT_CONFIG} to {CONFIG}"))?;
            println!("Created {CONFIG} from {DEFAULT_CONFIG}");
        }

        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read {CONFIG}"))?;

        serde_yaml::from_str(&content).with_context(|| format!("Failed to parse {CONFIG}"))
    }
}
