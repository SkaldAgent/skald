use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::Utc;
use serde::Serialize;
use tokio::sync::{broadcast, Mutex, oneshot};
use tracing::info;

use crate::core::events::{GlobalEvent, ServerEvent};

#[derive(Debug, Clone, Serialize)]
pub struct PendingClarificationInfo {
    pub request_id:        i64,
    pub session_id:        i64,
    pub agent_id:          String,
    pub source:            String,
    pub context_label:     Option<String>,
    pub title:             String,
    pub question:          String,
    pub suggested_answers: Vec<String>,
    pub created_at:        String,
}

struct PendingEntry {
    info: PendingClarificationInfo,
    tx:   oneshot::Sender<String>,
}

pub struct ClarificationManager {
    pending:  Mutex<HashMap<i64, PendingEntry>>,
    next_id:  AtomicI64,
    /// Global event bus sender, mirroring `ApprovalManager`. Used to broadcast
    /// `ClarificationRequested` / `ClarificationResolved` so Inbox subscribers
    /// (e.g. the mobile-connector plugin) can re-snapshot.
    event_tx: broadcast::Sender<GlobalEvent>,
}

impl ClarificationManager {
    pub fn new(event_tx: broadcast::Sender<GlobalEvent>) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            event_tx,
        })
    }

    pub async fn register(
        &self,
        session_id:        i64,
        agent_id:          &str,
        source:            &str,
        context_label:     Option<&str>,
        title:             &str,
        question:          &str,
        suggested_answers: Vec<String>,
    ) -> (i64, oneshot::Receiver<String>) {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx)   = oneshot::channel();

        let entry = PendingEntry {
            info: PendingClarificationInfo {
                request_id,
                session_id,
                agent_id:      agent_id.to_string(),
                source:        source.to_string(),
                context_label: context_label.map(str::to_string),
                title:         title.to_string(),
                question:      question.to_string(),
                suggested_answers,
                created_at:    Utc::now().to_rfc3339(),
            },
            tx,
        };

        let title_owned  = title.to_string();
        let source_owned = source.to_string();
        self.pending.lock().await.insert(request_id, entry);
        info!(session_id, agent = agent_id, source, request_id, "clarification: pending registered");
        // Broadcast on the global bus; counterpart of the per-session
        // `AgentQuestion` WS event.
        let _ = self.event_tx.send(GlobalEvent {
            source:     Some(source_owned),
            session_id: Some(session_id),
            event:      ServerEvent::ClarificationRequested {
                request_id,
                title: title_owned,
            },
        });
        (request_id, rx)
    }

    pub async fn resolve(&self, request_id: i64, answer: String) -> bool {
        if let Some(entry) = self.pending.lock().await.remove(&request_id) {
            info!(request_id, "clarification: resolved");
            let session_id = entry.info.session_id;
            let source     = entry.info.source.clone();
            let _ = entry.tx.send(answer);
            let _ = self.event_tx.send(GlobalEvent {
                source:     Some(source),
                session_id: Some(session_id),
                event:      ServerEvent::ClarificationResolved { request_id },
            });
            true
        } else {
            false
        }
    }

    pub async fn list_pending(&self) -> Vec<PendingClarificationInfo> {
        let guard = self.pending.lock().await;
        let mut items: Vec<_> = guard.values().map(|e| e.info.clone()).collect();
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        items
    }

    pub async fn cancel_for_session(&self, session_id: i64) {
        self.pending.lock().await.retain(|_, e| e.info.session_id != session_id);
    }
}
