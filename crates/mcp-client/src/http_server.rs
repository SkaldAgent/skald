use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::{McpServerClient, McpTool, extract_text, interpolate_env};
use crate::config::McpServerConfig;

const CALL_TIMEOUT_SECS: u64 = 120;

pub struct McpHttpServer {
    name:       String,
    url:        String,
    client:     reqwest::Client,
    headers:    HeaderMap,
    /// Set after the `initialize` response — required by stateful servers like Tavily.
    session_id: Mutex<Option<String>>,
    next_id:    AtomicU64,
    tools:      Vec<McpTool>,
}

impl McpHttpServer {
    pub async fn start(cfg: &McpServerConfig) -> Result<Self> {
        let url = cfg.url.as_deref()
            .ok_or_else(|| anyhow::anyhow!("http server '{}' requires 'url'", cfg.name))?
            .trim_end_matches('/')
            .to_string();

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/event-stream"));

        if let Some(key) = &cfg.api_key {
            let val = interpolate_env(key);
            let bearer = format!("Bearer {val}");
            headers.insert(AUTHORIZATION, bearer.parse()
                .map_err(|_| anyhow::anyhow!("invalid api_key for '{}'", cfg.name))?);
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(CALL_TIMEOUT_SECS))
            .build()?;

        let server = McpHttpServer {
            name:       cfg.name.clone(),
            url,
            client,
            headers,
            session_id: Mutex::new(None),
            next_id:    AtomicU64::new(1),
            tools:      Vec::new(),
        };

        server.request("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities":    {},
            "clientInfo":      { "name": "skald", "version": env!("CARGO_PKG_VERSION") },
        })).await?;

        if let Err(e) = server.notify("notifications/initialized", json!({})).await {
            warn!("MCP http '{}': initialized notification failed (ignoring): {e}", server.name);
        }

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
                    "type": "object", "properties": {},
                })),
            })
            .collect();

        Ok(McpHttpServer { tools, ..server })
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
        let body = json!({
            "jsonrpc": "2.0",
            "id":      id,
            "method":  method,
            "params":  params,
        });

        let mut req_headers = self.headers.clone();
        if let Some(sid) = self.session_id.lock().unwrap().as_deref() {
            if let Ok(val) = HeaderValue::from_str(sid) {
                req_headers.insert("mcp-session-id", val);
            }
        }

        let resp = self.client
            .post(&self.url)
            .headers(req_headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("MCP http '{}' request failed: {e}", self.name))?;

        if let Some(sid) = resp.headers().get("mcp-session-id") {
            if let Ok(sid_str) = sid.to_str() {
                debug!("MCP http '{}': captured session id", self.name);
                *self.session_id.lock().unwrap() = Some(sid_str.to_string());
            }
        }

        let status = resp.status();
        let content_type = resp.headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let msg: Value = if content_type.contains("text/event-stream") {
            parse_sse_response(resp).await
                .map_err(|e| anyhow::anyhow!("MCP http '{}' SSE parse error: {e}", self.name))?
        } else {
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("MCP http '{}' HTTP {status}: {body}", self.name);
            }
            resp.json::<Value>().await
                .map_err(|e| anyhow::anyhow!("MCP http '{}' JSON decode error: {e}", self.name))?
        };

        if let Some(error) = msg.get("error") {
            anyhow::bail!("MCP http '{}' protocol error: {error}", self.name);
        }
        Ok(msg["result"].clone())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let body = json!({
            "jsonrpc": "2.0",
            "method":  method,
            "params":  params,
        });

        let mut req_headers = self.headers.clone();
        if let Some(sid) = self.session_id.lock().unwrap().as_deref() {
            if let Ok(val) = HeaderValue::from_str(sid) {
                req_headers.insert("mcp-session-id", val);
            }
        }

        self.client
            .post(&self.url)
            .headers(req_headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("MCP http notify failed: {e}"))?;
        Ok(())
    }
}

#[async_trait]
impl McpServerClient for McpHttpServer {
    fn tools(&self) -> &[McpTool] { self.tools() }
    async fn call_tool(&self, name: &str, args: Value) -> Result<String> { self.call_tool(name, args).await }
}

async fn parse_sse_response(resp: reqwest::Response) -> Result<Value> {
    let text = resp.text().await?;
    for line in text.lines() {
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None    => continue,
        };
        if data == "[DONE]" { break; }
        if let Ok(msg) = serde_json::from_str::<Value>(data) {
            if msg.get("result").is_some() || msg.get("error").is_some() {
                return Ok(msg);
            }
        }
    }
    anyhow::bail!("no JSON-RPC result found in SSE response")
}
