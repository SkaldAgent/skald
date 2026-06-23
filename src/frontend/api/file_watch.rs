//! File-watch WebSocket endpoint.
//!
//! `GET /api/file/watch` upgrades to a long-lived WebSocket. The client sends
//! JSON commands:
//!
//! ```jsonc
//! { "op": "subscribe",   "path": "docs/index.md" }   // start watching
//! { "op": "unsubscribe", "path": "docs/index.md" }   // stop watching
//! ```
//!
//! The server pushes change notifications:
//!
//! ```jsonc
//! { "type": "subscribed",   "path": "..." }   // ack after a successful subscribe
//! { "type": "unsubscribed", "path": "..." }   // ack after an unsubscribe
//! { "type": "changed",      "path": "..." }   // file changed on disk
//! { "type": "error",        "path": "...", "error": "..." }   // watch install failed
//! ```
//!
//! `path` is the original user-supplied string (relative or absolute) — it
//! round-trips unchanged so the client can match it against the path it asked
//! to watch. The backend resolves it to an absolute path via `fs_tools::resolve`
//! (same path model as `GET /api/file`), so absolute paths are used as-is and
//! relative paths resolve against Skald's process CWD (the data root).
//!
//! One OS watcher per subscription per connection (no cross-connection
//! sharing). On disconnect every watcher is dropped and the OS resources are
//! released automatically.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::info;

use crate::core::skald::Skald;
use crate::core::tools::fs as fs_tools;

pub async fn handler(
    ws: WebSocketUpgrade,
    State(_skald): State<Arc<Skald>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket))
}

#[derive(Deserialize)]
struct ClientMsg {
    op: String,
    path: String,
}

async fn handle_socket(mut socket: WebSocket) {
    info!("file-watch WS connected");

    // Single mpsc into which every watcher callback forwards via an unbounded
    // sender (unbounded so the sync callback never blocks).
    let (change_tx, mut change_rx) = mpsc::unbounded_channel::<String>();

    // original_path -> watcher (dropping the watcher un-watches the path).
    let mut watchers: HashMap<String, RecommendedWatcher> = HashMap::new();

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let parsed = match serde_json::from_str::<ClientMsg>(&text) {
                            Ok(p) => p,
                            Err(e) => {
                                let _ = send_json(&mut socket,
                                    json!({ "type": "error", "error": format!("bad message: {e}") })
                                ).await;
                                continue;
                            }
                        };
                        match parsed.op.as_str() {
                            "subscribe" => {
                                if watchers.contains_key(&parsed.path) {
                                    // Already subscribed — silently ack.
                                    let _ = send_json(&mut socket,
                                        json!({ "type": "subscribed", "path": parsed.path })
                                    ).await;
                                    continue;
                                }
                                if let Err(err) = install_watcher(&parsed.path, &change_tx, &mut watchers) {
                                    let _ = send_json(&mut socket,
                                        json!({ "type": "error", "path": parsed.path, "error": err })
                                    ).await;
                                } else {
                                    let _ = send_json(&mut socket,
                                        json!({ "type": "subscribed", "path": parsed.path })
                                    ).await;
                                }
                            }
                            "unsubscribe" => {
                                if watchers.remove(&parsed.path).is_some() {
                                    let _ = send_json(&mut socket,
                                        json!({ "type": "unsubscribed", "path": parsed.path })
                                    ).await;
                                }
                            }
                            other => {
                                let _ = send_json(&mut socket,
                                    json!({ "type": "error", "error": format!("unknown op: {other}") })
                                ).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }

            changed_path = change_rx.recv() => {
                if let Some(p) = changed_path {
                    if send_json(&mut socket, json!({ "type": "changed", "path": p })).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    info!("file-watch WS disconnected");
}

/// Create a `RecommendedWatcher` for `user_path`, install it, and store it in
/// `watchers` keyed by the original (un-resolved) path. Returns an error
/// string on failure so the caller can report it to the client.
fn install_watcher(
    user_path: &str,
    change_tx: &mpsc::UnboundedSender<String>,
    watchers: &mut HashMap<String, RecommendedWatcher>,
) -> Result<(), String> {
    let abs: PathBuf = fs_tools::resolve(user_path).map_err(|e| e.to_string())?;
    let tx_for_cb = change_tx.clone();
    let original_path = user_path.to_string();

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            // Any event on the watched path triggers a change notification.
            // We don't inspect the event kind — reload on the client side
            // re-reads the file and naturally handles create/modify/remove.
            if res.is_ok() {
                if tx_for_cb.send(original_path.clone()).is_err() {
                    // channel closed — receiver dropped (WS disconnected).
                }
            }
        },
        Config::default(),
    )
    .map_err(|e| format!("watcher create failed: {e}"))?;

    watcher
        .watch(&abs, RecursiveMode::NonRecursive)
        .map_err(|e| format!("watch install failed: {e}"))?;

    watchers.insert(user_path.to_string(), watcher);
    Ok(())
}

async fn send_json(socket: &mut WebSocket, value: serde_json::Value) -> Result<(), axum::Error> {
    socket.send(Message::Text(value.to_string().into())).await
}
