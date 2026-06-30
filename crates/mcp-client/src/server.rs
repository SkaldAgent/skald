use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, warn};

use crate::{McpCallResult, McpServerClient, McpTool, extract_text, interpolate_env};
use crate::config::McpServerConfig;

/// A server-initiated notification from an MCP server: `(server_name, full JSON-RPC message)`.
pub type McpNotification = (String, Value);

const CALL_TIMEOUT_SECS: u64 = 120;

// ── Elicitation (MCP spec 2025-06-18) ──────────────────────────────────────────

/// A server-initiated elicitation request: the server asks the user for input
/// *during* a tool call (`elicitation/create`). The secret/value never reaches
/// the LLM and is never persisted.
#[derive(Debug, Clone)]
pub struct ElicitationRequest {
    /// Human-readable message to show the user.
    pub message:          String,
    /// JSON Schema (flat object of typed fields) describing the requested input.
    pub requested_schema: Value,
}

/// The user's decision on an elicitation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElicitationAction {
    Accept,
    Decline,
    Cancel,
}

impl ElicitationAction {
    fn as_str(self) -> &'static str {
        match self {
            ElicitationAction::Accept  => "accept",
            ElicitationAction::Decline => "decline",
            ElicitationAction::Cancel  => "cancel",
        }
    }
}

/// The reply sent back to the server for an `elicitation/create` request.
#[derive(Debug, Clone)]
pub struct ElicitationReply {
    pub action:  ElicitationAction,
    /// Field values for `accept`; `None` for `decline`/`cancel`.
    pub content: Option<Value>,
}

/// Bridges a server-initiated elicitation to whatever surfaces it to the user
/// (in Skald: the Agent Inbox via `ElicitationManager`). The crate writes the
/// JSON-RPC reply itself — the handler only produces the decision. The returned
/// value (and any secret it carries) flows straight to the server's stdin and is
/// never logged here.
#[async_trait]
pub trait ElicitationHandler: Send + Sync {
    async fn handle(&self, server_name: &str, request: ElicitationRequest) -> ElicitationReply;
}

/// Serialises `msg` as a single JSON-RPC line and writes it to the child's stdin.
async fn write_json_line(stdin: &Arc<Mutex<ChildStdin>>, msg: &Value) {
    if let Ok(mut line) = serde_json::to_string(msg) {
        line.push('\n');
        let _ = stdin.lock().await.write_all(line.as_bytes()).await;
    }
}

/// Handles a server→client JSON-RPC request (e.g. `elicitation/create`) without
/// blocking the read-loop: the user reply may take minutes, so the work is
/// spawned and the JSON-RPC response is written back when it resolves.
fn handle_server_request(
    server_name:          &str,
    msg:                  Value,
    stdin:                &Arc<Mutex<ChildStdin>>,
    handler:              &Option<Arc<dyn ElicitationHandler>>,
    pending_elicitations: &Arc<AtomicUsize>,
) {
    let id     = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    if method == "elicitation/create" {
        let params  = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let request = ElicitationRequest {
            message:          params.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
            requested_schema: params.get("requestedSchema").cloned().unwrap_or_else(|| json!({})),
        };
        let stdin = Arc::clone(stdin);

        let Some(handler) = handler.clone() else {
            // Capability declared but no handler wired: cancel so the server
            // doesn't hang waiting for input we can't collect.
            tokio::spawn(async move {
                write_json_line(&stdin, &json!({
                    "jsonrpc": "2.0", "id": id, "result": { "action": "cancel" },
                })).await;
            });
            return;
        };

        pending_elicitations.fetch_add(1, Ordering::SeqCst);
        let counter = Arc::clone(pending_elicitations);
        let server  = server_name.to_string();
        tokio::spawn(async move {
            let reply      = handler.handle(&server, request).await;
            let mut result = json!({ "action": reply.action.as_str() });
            if reply.action == ElicitationAction::Accept {
                if let Some(content) = reply.content {
                    result["content"] = content;
                }
            }
            write_json_line(&stdin, &json!({
                "jsonrpc": "2.0", "id": id, "result": result,
            })).await;
            counter.fetch_sub(1, Ordering::SeqCst);
        });
    } else {
        // Unknown server→client request: reply method-not-found so the server
        // isn't left hanging.
        let stdin  = Arc::clone(stdin);
        let method = method.to_string();
        tokio::spawn(async move {
            write_json_line(&stdin, &json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") },
            })).await;
        });
    }
}

pub struct McpServer {
    name:    String,
    stdin:   Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: AtomicU64,
    tools:   Vec<McpTool>,
    /// Number of in-flight server→client elicitations awaiting a user reply.
    /// While > 0, an in-flight `tools/call` on this server must not time out
    /// (the user may still be typing a password into the Inbox).
    pending_elicitations: Arc<AtomicUsize>,
}

impl McpServer {
    pub async fn start(
        cfg: &McpServerConfig,
        notification_tx: Option<mpsc::UnboundedSender<McpNotification>>,
        elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
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
           // Capture the child's stderr instead of inheriting it: many MCP
           // servers (e.g. FastMCP) print a startup banner, deprecation
           // warnings and INFO logs there, which would otherwise spill raw
           // onto our console. We drain it into tracing at `debug` level so it
           // stays quiet by default but is still available for diagnostics.
           .stderr(Stdio::piped())
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
        // Wrap stdin early so both the struct and the read-loop (which writes
        // elicitation replies back to the server) can share the same handle.
        let stdin = Arc::new(Mutex::new(stdin));
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("could not capture stdout for '{}'", cfg.name))?;

        // Drain the child's stderr into tracing at `debug` so banners/warnings
        // from MCP servers don't pollute our console at the default log level.
        if let Some(stderr) = child.stderr.take() {
            let server_name_err = cfg.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        debug!(target: "mcp_client", "[{server_name_err}] {line}");
                    }
                }
            });
        }

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_elicitations = Arc::new(AtomicUsize::new(0));

        let pending_bg               = pending.clone();
        let server_name_bg           = cfg.name.clone();
        let notification_tx_bg       = notification_tx;
        let stdin_bg                 = Arc::clone(&stdin);
        let elicitation_handler_bg   = elicitation_handler;
        let pending_elicitations_bg  = Arc::clone(&pending_elicitations);
        tokio::spawn(async move {
            let mut child = child;
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                            let has_method = msg.get("method").is_some();
                            let has_id     = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
                            if has_method && has_id {
                                // Server → client request (e.g. elicitation/create):
                                // has *both* `method` and `id`, so it must be checked
                                // before the response branch below.
                                handle_server_request(
                                    &server_name_bg, msg, &stdin_bg,
                                    &elicitation_handler_bg, &pending_elicitations_bg,
                                );
                            } else if let Some(id) = msg["id"].as_u64() {
                                if let Some(tx) = pending_bg.lock().await.remove(&id) {
                                    let _ = tx.send(msg);
                                }
                            } else if has_method {
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
            stdin,
            pending,
            next_id: AtomicU64::new(1),
            tools:   Vec::new(),
            pending_elicitations,
        };

        let init = server.request("initialize", json!({
            // Declare the elicitation capability (form mode) so servers know they
            // may request input mid-call.
            "protocolVersion": crate::PROTOCOL_VERSION,
            "capabilities":    { "elicitation": {} },
            "clientInfo":      { "name": "skald", "version": env!("CARGO_PKG_VERSION") },
        })).await?;
        // Tolerate a server that negotiates a different (older) version — warn but
        // keep going rather than disconnecting.
        if let Some(v) = init["protocolVersion"].as_str() {
            if v != crate::PROTOCOL_VERSION {
                warn!("MCP '{}': server negotiated protocol {v} (we requested {}); proceeding",
                    server.name, crate::PROTOCOL_VERSION);
            }
        }

        server.notify("notifications/initialized", json!({})).await?;

        // Follow `nextCursor` across pages so servers with large tool lists aren't
        // silently truncated; capped at `MAX_TOOL_PAGES` against a stuck cursor.
        let mut tools: Vec<McpTool> = Vec::new();
        let mut cursor: Option<String> = None;
        for page_n in 0..crate::MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None    => json!({}),
            };
            let page = server.request("tools/list", params).await?;
            if let Some(arr) = page["tools"].as_array() {
                tools.extend(arr.iter().map(|t| McpTool::from_json(&cfg.name, t)));
            }
            cursor = page["nextCursor"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
            if page_n + 1 == crate::MAX_TOOL_PAGES {
                warn!("MCP '{}': tools/list hit {}-page cap; some tools may be omitted",
                    server.name, crate::MAX_TOOL_PAGES);
            }
        }

        Ok(McpServer { tools, ..server })
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<McpCallResult> {
        let result = self.request("tools/call", json!({
            "name":      name,
            "arguments": args,
        })).await?;

        if result["isError"].as_bool().unwrap_or(false) {
            anyhow::bail!("MCP tool error: {}", extract_text(&result));
        }
        Ok(crate::extract_call_result(&result))
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

        // Wait for the response, but re-arm the timeout while an elicitation is
        // in flight on this server: the user may still be typing a secret into
        // the Inbox, and the server won't answer `tools/call` until then.
        tokio::pin!(rx);
        let response = loop {
            tokio::select! {
                res = &mut rx => break res
                    .map_err(|_| anyhow::anyhow!("MCP '{}' disconnected", self.name))?,
                _ = tokio::time::sleep(Duration::from_secs(CALL_TIMEOUT_SECS)) => {
                    if self.pending_elicitations.load(Ordering::SeqCst) == 0 {
                        anyhow::bail!("MCP '{}' timed out on '{method}'", self.name);
                    }
                    // Elicitation pending: keep waiting for the user.
                }
            }
        };

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
    async fn call_tool(&self, name: &str, args: Value) -> Result<McpCallResult> { self.call_tool(name, args).await }
}
