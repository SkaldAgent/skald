mod core;
mod frontend;
mod config;

use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use core_api::plugin::Plugin;
use config::Config;
use crate::core::skald::Skald;
use crate::frontend::WebFrontend;

const APP_NAME: &str = env!("CARGO_PKG_NAME");

fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<()> {
    std::fs::create_dir_all("logs")?;
    let file_appender = tracing_appender::rolling::daily("logs", format!("{APP_NAME}.log"));
    let (non_blocking, _log_guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    info!(version = env!("CARGO_PKG_VERSION"), "starting {APP_NAME}");

    let config = match Config::load() {
        Ok(c)  => { debug!("config loaded"); c }
        Err(e) => { error!(error = %e, "failed to load config"); return Err(e); }
    };
    let (core_cfg, frontend_cfg) = config.into_split();

    let mut plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(plugin_honcho::HonchoPlugin::new()),
        Arc::new(plugin_telegram_bot::TelegramPlugin::new("secrets")),
        Arc::new(plugin_tailscale_remote::RemotePlugin::new()),
        Arc::new(plugin_comfyui::ComfyUIPlugin::new()),
        Arc::new(plugin_tts_orpheus_3b::OrpheusTtsPlugin::new()),
        Arc::new(plugin_tts_kokoro::KokoroTtsPlugin::new()),
        Arc::new(plugin_elevenlabs::ElevenLabsPlugin::new()),
    ];
    #[cfg(feature = "whisper-local")]
    plugins.push(Arc::new(plugin_transcribe_whisper_local::WhisperLocalPlugin::new()));

    let skald = Skald::new(&core_cfg, plugins).await?;

    let handle = WebFrontend::new(skald.clone(), &frontend_cfg).start().await?;

    tokio::signal::ctrl_c().await?;
    warn!("SIGINT received — shutting down");

    handle.shutdown().await;
    skald.shutdown().await;
    info!("shutdown complete");

    Ok(())
}
