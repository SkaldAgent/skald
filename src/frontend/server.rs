use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use tokio::{net::TcpListener, task::JoinHandle};
use tower_http::services::ServeDir;

use crate::frontend::api;
use crate::core::skald::Skald;

pub struct WebServer {
    static_dir: String,
    skald:      Arc<Skald>,
}

pub struct WebServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    task:        JoinHandle<()>,
}

impl WebServer {
    pub fn new(static_dir: String, skald: Arc<Skald>) -> Self {
        Self { static_dir, skald }
    }

    pub async fn start(self, addr: &str) -> Result<WebServerHandle> {
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("Failed to bind to {addr}"))?;

        let local_addr = listener.local_addr()?;
        println!("Server running at http://{local_addr}/");

        let router = Self::build_router(&self.static_dir, self.skald);

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

    pub fn build_router(static_dir: &str, skald: Arc<Skald>) -> Router {
        Router::new()
            .nest("/api", api::router())
            .with_state(skald)
            .fallback_service(ServeDir::new(static_dir))
    }
}

impl WebServerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.task.await;
    }
}
