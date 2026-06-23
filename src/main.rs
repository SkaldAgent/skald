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
use crate::core::db::init_pool;
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
        Arc::new(plugin_mobile_connector::MobileConnectorPlugin::new()),
    ];
    #[cfg(feature = "whisper-local")]
    plugins.push(Arc::new(plugin_transcribe_whisper_local::WhisperLocalPlugin::new()));

    let pool = std::sync::Arc::new(init_pool(&core_cfg.db.path).await?);
    info!(path = %core_cfg.db.path, "database ready");

    let skald = Skald::new(std::sync::Arc::clone(&pool), &core_cfg, plugins).await?;

    let handle = WebFrontend::new(skald.clone(), std::sync::Arc::clone(&pool), &frontend_cfg).start().await?;

    let signal = wait_for_shutdown_signal().await;
    warn!(signal, "shutdown signal received — shutting down");

    handle.shutdown().await;
    skald.shutdown().await;
    pool.close().await;
    info!("shutdown complete");

    Ok(())
}

/// Wait for an OS shutdown signal and return its name for logging.
///
/// We trap **both** SIGINT (Ctrl+C) and SIGTERM. Without an explicit SIGTERM
/// handler the default action kills the process with exit code 143, which the
/// `run.sh` supervisor treats as a hard stop (only exit 255 triggers a
/// restart) — and the kill leaves no trace in the log. Trapping it lets us log
/// the cause and shut down gracefully (exit 0).
#[cfg(unix)]
async fn wait_for_shutdown_signal() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = sigterm.recv() => "SIGTERM",
        _ = sigint.recv()  => "SIGINT",
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> &'static str {
    let _ = tokio::signal::ctrl_c().await;
    "CTRL_C"
}
