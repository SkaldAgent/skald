use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use sqlx::SqlitePool;
use tokio::{net::TcpListener, sync::RwLock, task::JoinHandle};
use tower_http::services::ServeDir;

use crate::api;
use crate::approval::ApprovalManager;
use crate::chat_event_bus::ChatEventBus;
use crate::chat_hub::ChatHub;
use crate::clarification::ClarificationManager;
use crate::config::ServerConfig;
use crate::cron::CronTaskManager;
use crate::location::LocationManager;
use crate::mcp::McpManager;
use crate::memory::MemoryManager;
use crate::plugin::PluginManager;
use core_api::remote::RemoteAccess;
use crate::session::manager::ChatSessionManager;
use crate::tic::TicManager;
use crate::tools::ToolRegistry;
use crate::image_generate::ImageGeneratorManager;
use crate::secrets::SecretsStore;
use crate::transcribe::TranscribeManager;
use crate::tts::TtsManager;


#[derive(Clone)]
pub struct AppState {
    pub manager:            Arc<ChatSessionManager>,
    pub chat_hub:           Arc<ChatHub>,
    pub db:                 Arc<SqlitePool>,
    pub mcp:                Arc<McpManager>,
    pub cron:               Arc<CronTaskManager>,
    pub plugin_manager:     Arc<PluginManager>,
    pub location_manager:   Arc<LocationManager>,
    pub approval:           Arc<ApprovalManager>,
    pub clarification:      Arc<ClarificationManager>,
    pub tools:              Arc<ToolRegistry>,
    pub secrets:                  Arc<SecretsStore>,
    pub transcribe_manager:       Arc<TranscribeManager>,
    pub tts_manager:              Arc<TtsManager>,
    pub image_generator_manager:  Arc<ImageGeneratorManager>,
    pub tic_manager:        Arc<TicManager>,
    pub event_bus:          Arc<ChatEventBus>,
    pub memory_manager:     Arc<MemoryManager>,
    /// Active remote-connectivity provider (e.g. Tailscale).
    /// None when the remote_connectivity plugin is disabled or not yet started.
    pub remote:             Arc<RwLock<Option<Arc<dyn RemoteAccess>>>>,
    /// Static file directory served by the web server — used by the remote plugin
    /// to reconstruct the Axum router for the mesh-facing server.
    pub web_static_dir:     Arc<str>,
    /// Port the local HTTP server listens on — used by the remote plugin to bind
    /// on the same port on the mesh interface.
    pub web_port:           u16,
}

pub struct WebServer {
    config:     ServerConfig,
    static_dir: String,
    state:      AppState,
}

pub struct WebServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    task:        JoinHandle<()>,
}

impl WebServer {
    pub fn new(config: ServerConfig, static_dir: String, state: AppState) -> Self {
        Self { config, static_dir, state }
    }

    pub async fn start(self) -> Result<WebServerHandle> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("Failed to bind to {addr}"))?;

        let local_addr = listener.local_addr()?;
        println!("Server running at http://{local_addr}/");

        let router = Self::build_router(&self.static_dir, self.state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("Web server encountered a fatal error");
        });

        Ok(WebServerHandle { shutdown_tx, task })
    }

    pub fn build_router(static_dir: &str, state: AppState) -> Router {
        Router::new()
            .nest("/api", api::router())
            .with_state(state)
            .fallback_service(ServeDir::new(static_dir))
    }
}

impl WebServerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.task.await;
    }
}
