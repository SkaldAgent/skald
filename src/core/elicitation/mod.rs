//! Elicitation — server-initiated input requests (MCP spec 2025-06-18).
//!
//! When an MCP server needs input *during* a tool call (e.g. a sudo password),
//! it sends `elicitation/create`. The `mcp-client` read-loop forwards it through
//! the [`ElicitationHandler`] bridge to the [`ElicitationManager`], which surfaces
//! it in the Agent Inbox and waits for the user's decision. The reply (and any
//! secret it carries) flows straight back to the server's stdin — it is **never**
//! logged, broadcast in an event, or written to the DB.
//!
//! Mirrors [`crate::core::clarification`], but with the `accept`/`decline`/`cancel`
//! outcome and a `sensitive` flag that elicitation needs and clarification lacks.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::{broadcast, Mutex, oneshot};
use tracing::{debug, info};

use mcp_client::{ElicitationAction, ElicitationHandler, ElicitationReply, ElicitationRequest};

use crate::core::events::{GlobalEvent, ServerEvent};

/// How long the user has to answer an elicitation before we reply `cancel`.
/// Independent of any secret-cache TTL the MCP server keeps in its own RAM.
const ELICITATION_DEADLINE: Duration = Duration::from_secs(300);

/// One pending elicitation, surfaced to the Inbox UI. Holds **no value** — only
/// the prompt metadata. The secret travels through the `oneshot`, not here.
#[derive(Debug, Clone, Serialize)]
pub struct PendingElicitationInfo {
    pub request_id:      i64,
    pub server_name:     String,
    pub message:         String,
    /// Name of the single requested field (v1 supports one field), if any.
    pub field_name:      Option<String>,
    /// Render the input masked (`<input type="password">`) and never echo it.
    pub sensitive:       bool,
    /// Empty `requestedSchema` ⇒ pure yes/no confirmation (no input field).
    pub is_confirmation: bool,
    pub created_at:      String,
}

/// The user's decision, fed back from the Inbox API into the waiting handler.
#[derive(Debug, Clone)]
pub struct ElicitationOutcome {
    /// `"accept"` | `"decline"` | `"cancel"`.
    pub action:  String,
    /// Field values for `accept` (e.g. `{ "password": "…" }`); `None` otherwise.
    pub content: Option<Value>,
}

struct PendingEntry {
    info: PendingElicitationInfo,
    tx:   oneshot::Sender<ElicitationOutcome>,
}

pub struct ElicitationManager {
    pending:  Mutex<HashMap<i64, PendingEntry>>,
    next_id:  AtomicI64,
    /// Global event bus, mirroring `ClarificationManager`. Broadcasts
    /// `ElicitationRequested` / `ElicitationResolved` so Inbox subscribers
    /// re-snapshot. **Never** carries the secret — only `request_id` + title.
    event_tx: broadcast::Sender<GlobalEvent>,
}

impl ElicitationManager {
    pub fn new(event_tx: broadcast::Sender<GlobalEvent>) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            event_tx,
        })
    }

    /// Register a pending elicitation derived from an `elicitation/create`
    /// request. Returns the id and a receiver that resolves when the user
    /// answers (via the Inbox API) or the request is cancelled.
    pub async fn register(
        &self,
        server_name:      &str,
        message:          &str,
        requested_schema: &Value,
    ) -> (i64, oneshot::Receiver<ElicitationOutcome>) {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx)   = oneshot::channel();

        let (field_name, sensitive, is_confirmation) = parse_schema(requested_schema);
        let info = PendingElicitationInfo {
            request_id,
            server_name:     server_name.to_string(),
            message:         message.to_string(),
            field_name,
            sensitive,
            is_confirmation,
            created_at:      Utc::now().to_rfc3339(),
        };

        let title = if message.is_empty() {
            format!("{server_name}: input requested")
        } else {
            message.to_string()
        };

        self.pending.lock().await.insert(request_id, PendingEntry { info, tx });
        info!(server = server_name, request_id, sensitive, "elicitation: pending registered");
        let _ = self.event_tx.send(GlobalEvent {
            source:     None,
            session_id: None,
            event:      ServerEvent::ElicitationRequested { request_id, title },
        });
        (request_id, rx)
    }

    /// Resolve a pending elicitation with the user's decision. The `content`
    /// (which may hold a secret) is forwarded on the `oneshot` and never logged.
    pub async fn resolve(&self, request_id: i64, outcome: ElicitationOutcome) -> bool {
        if let Some(entry) = self.pending.lock().await.remove(&request_id) {
            debug!(request_id, action = %outcome.action, "elicitation: resolved");
            let _ = entry.tx.send(outcome);
            self.broadcast_resolved(request_id);
            true
        } else {
            false
        }
    }

    /// Drop a pending elicitation without a user answer (deadline elapsed or the
    /// waiting handler went away). The dropped `oneshot` sender makes the handler
    /// reply `cancel`.
    pub async fn cancel(&self, request_id: i64) {
        if self.pending.lock().await.remove(&request_id).is_some() {
            debug!(request_id, "elicitation: cancelled (deadline/handler gone)");
            self.broadcast_resolved(request_id);
        }
    }

    pub async fn list_pending(&self) -> Vec<PendingElicitationInfo> {
        let guard = self.pending.lock().await;
        let mut items: Vec<_> = guard.values().map(|e| e.info.clone()).collect();
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        items
    }

    fn broadcast_resolved(&self, request_id: i64) {
        let _ = self.event_tx.send(GlobalEvent {
            source:     None,
            session_id: None,
            event:      ServerEvent::ElicitationResolved { request_id },
        });
    }
}

/// Derives, from an MCP `requestedSchema`, the single field name, whether it is
/// sensitive (masked input), and whether it is a pure confirmation (empty schema).
/// v1 supports exactly one field — extra properties are ignored.
fn parse_schema(schema: &Value) -> (Option<String>, bool, bool) {
    match schema.get("properties").and_then(Value::as_object) {
        Some(props) if !props.is_empty() => {
            let (key, def) = props.iter().next().unwrap();
            let format     = def.get("format").and_then(Value::as_str).unwrap_or("");
            let write_only = def.get("writeOnly").and_then(Value::as_bool).unwrap_or(false);
            let name_l     = key.to_lowercase();
            let sensitive  = format == "password"
                || write_only
                || ["password", "passphrase", "secret", "token"]
                    .iter()
                    .any(|s| name_l.contains(s));
            (Some(key.clone()), sensitive, false)
        }
        // No fields ⇒ confirmation request.
        _ => (None, false, true),
    }
}

/// Bridges `mcp-client`'s server→client elicitation to the `ElicitationManager`.
/// Registers the request, waits up to [`ELICITATION_DEADLINE`] for the user, and
/// maps the outcome back to an [`ElicitationReply`].
pub struct ElicitationBridge {
    manager: Arc<ElicitationManager>,
}

impl ElicitationBridge {
    pub fn new(manager: Arc<ElicitationManager>) -> Arc<Self> {
        Arc::new(Self { manager })
    }
}

#[async_trait]
impl ElicitationHandler for ElicitationBridge {
    async fn handle(&self, server_name: &str, request: ElicitationRequest) -> ElicitationReply {
        let (id, rx) = self
            .manager
            .register(server_name, &request.message, &request.requested_schema)
            .await;

        match tokio::time::timeout(ELICITATION_DEADLINE, rx).await {
            Ok(Ok(outcome)) => {
                let action = match outcome.action.as_str() {
                    "accept"  => ElicitationAction::Accept,
                    "decline" => ElicitationAction::Decline,
                    _         => ElicitationAction::Cancel,
                };
                let content = if action == ElicitationAction::Accept { outcome.content } else { None };
                ElicitationReply { action, content }
            }
            // Deadline elapsed or the resolver's sender was dropped → cancel.
            _ => {
                self.manager.cancel(id).await;
                ElicitationReply { action: ElicitationAction::Cancel, content: None }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_schema_is_confirmation() {
        let (field, sensitive, confirm) = parse_schema(&json!({ "type": "object", "properties": {} }));
        assert_eq!(field, None);
        assert!(!sensitive);
        assert!(confirm);
    }

    #[test]
    fn missing_properties_is_confirmation() {
        let (field, _sensitive, confirm) = parse_schema(&json!({ "type": "object" }));
        assert_eq!(field, None);
        assert!(confirm);
    }

    #[test]
    fn password_format_is_sensitive() {
        let schema = json!({ "type": "object", "properties": {
            "password": { "type": "string", "format": "password" }
        }});
        let (field, sensitive, confirm) = parse_schema(&schema);
        assert_eq!(field.as_deref(), Some("password"));
        assert!(sensitive);
        assert!(!confirm);
    }

    #[test]
    fn secret_by_name_is_sensitive() {
        let schema = json!({ "type": "object", "properties": {
            "api_token": { "type": "string" }
        }});
        let (_field, sensitive, _confirm) = parse_schema(&schema);
        assert!(sensitive);
    }

    #[test]
    fn plain_field_is_not_sensitive() {
        let schema = json!({ "type": "object", "properties": {
            "hostname": { "type": "string" }
        }});
        let (field, sensitive, confirm) = parse_schema(&schema);
        assert_eq!(field.as_deref(), Some("hostname"));
        assert!(!sensitive);
        assert!(!confirm);
    }
}
