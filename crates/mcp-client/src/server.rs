use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use tracing::warn;

use crate::{McpServerClient, McpTool, extract_text, interpolate_env};
use crate::config::McpServerConfig;

/// A server-initiated notification from an MCP server: `(server_name, full JSON-RPC message)`.
pub type McpNotification = (String, Value);

const CALL_TIMEOUT_SECS: u64 = 120;

pub struct McpServer {
    name:    String,
    stdin:   Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
    tools:   Vec<McpTool>,
}

impl McpServer {
    pub async fn start(
        cfg: &McpServerConfig,
        notification_tx: Option<mpsc::UnboundedSender<McpNotification>>,
    ) -> Result<Self> {
        let command = cfg.command.as_deref()
            .ok_or_else(|| anyhow::anyhow!("stdio server '{}' requires 'command'", cfg.name))?;

        let mut cmd = Command::new(command);
        if let Some(args) = &cfg.args {
            cmd.args(args);
        }
        if let Some(env_map) = &cfg.env {
            for (k, v) in env_map {
                cmd.env(k, interpolate_env(v));
            }
        }
        cmd.stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::inherit())
           .kill_on_drop(true);

        // Detach the child into its own process group so that a terminal
        // Ctrl+C (SIGINT delivered to the whole foreground process group)
        // does not reach it directly. Otherwise Python-based MCP servers
        // catch the SIGINT and dump a KeyboardInterrupt traceback to stderr.
        // Cleanup still happens via `kill_on_drop`: when the app shuts down
        // and the reader task is dropped, the child receives SIGKILL silently.
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn '{}': {e}", cfg.name))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("could not capture stdin for '{}'", cfg.name))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("could not capture stdout for '{}'", cfg.name))?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_bg          = pending.clone();
        let server_name_bg      = cfg.name.clone();
        let notification_tx_bg  = notification_tx;
        tokio::spawn(async move {
            let mut child = child;
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                            if let Some(id) = msg["id"].as_u64() {
                                if let Some(tx) = pending_bg.lock().await.remove(&id) {
                                    let _ = tx.send(msg);
                                }
                            } else if msg.get("method").is_some() {
                                if let Some(tx) = &notification_tx_bg {
                                    let _ = tx.send((server_name_bg.clone(), msg));
                                }
                            }
                        }
                    }
                    Ok(Some(_)) => {}
                    _ => break,
                }
            }
            let exit_info = match child.wait().await {
                Ok(status) if !status.success() => format!(
                    "process exited with {}",
                    status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
                ),
                _ => "process exited unexpectedly".into(),
            };
            let error_msg = format!("MCP '{}' disconnected: {exit_info}", server_name_bg);
            for (_, tx) in pending_bg.lock().await.drain() {
                let _ = tx.send(json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32000, "message": error_msg }
                }));
            }
        });

        let server = McpServer {
            name:    cfg.name.clone(),
            stdin:   Arc::new(Mutex::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            tools:   Vec::new(),
        };

        server.request("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities":    {},
            "clientInfo":      { "name": "skald", "version": env!("CARGO_PKG_VERSION") },
        })).await?;

        server.notify("notifications/initialized", json!({})).await?;

        let tools_result = server.request("tools/list", json!({})).await?;
        let tools: Vec<McpTool> = tools_result["tools"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|t| McpTool {
                server_name:  cfg.name.clone(),
                name:         t["name"].as_str().unwrap_or("").to_string(),
                description:  t["description"].as_str().unwrap_or("").to_string(),
                input_schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({
                    "type": "object",
                    "properties": {},
                })),
            })
            .collect();

        Ok(McpServer { tools, ..server })
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String> {
        let result = self.request("tools/call", json!({
            "name":      name,
            "arguments": args,
        })).await?;

        if result["isError"].as_bool().unwrap_or(false) {
            anyhow::bail!("MCP tool error: {}", extract_text(&result));
        }
        Ok(extract_text(&result))
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let msg = json!({
            "jsonrpc": "2.0",
            "id":      id,
            "method":  method,
            "params":  params,
        });
        let mut line = serde_json::to_string(&msg)?;
        line.push('\n');
        self.stdin.lock().await.write_all(line.as_bytes()).await?;

        let response = timeout(Duration::from_secs(CALL_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| anyhow::anyhow!("MCP '{}' timed out on '{method}'", self.name))?
            .map_err(|_| anyhow::anyhow!("MCP '{}' disconnected", self.name))?;

        if let Some(error) = response.get("error") {
            anyhow::bail!("MCP '{}' protocol error: {error}", self.name);
        }
        Ok(response["result"].clone())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method":  method,
            "params":  params,
        });
        let mut line = serde_json::to_string(&msg)?;
        line.push('\n');
        self.stdin.lock().await.write_all(line.as_bytes()).await?;
        Ok(())
    }
}

#[async_trait]
impl McpServerClient for McpServer {
    fn tools(&self) -> &[McpTool] { self.tools() }
    async fn call_tool(&self, name: &str, args: Value) -> Result<String> { self.call_tool(name, args).await }
}
