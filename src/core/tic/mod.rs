use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sqlx::SqlitePool;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::core::chat_hub::ChatHub;
use crate::core::config::TicConfig;
use crate::core::db::mcp_events;
use crate::core::session::manager::ChatSessionManager;

const TIC_SOURCE: &str = "tic";
const TIC_AGENT:  &str = "tic";

pub struct TicManager {
    db:          Arc<SqlitePool>,
    session_mgr: Arc<ChatSessionManager>,
    hub:         Arc<ChatHub>,
    config:      TicConfig,
    /// Guards against concurrent ticks (e.g. if a tick takes longer than the interval).
    running: AtomicBool,
}

impl TicManager {
    pub fn new(
        db:          Arc<SqlitePool>,
        session_mgr: Arc<ChatSessionManager>,
        hub:         Arc<ChatHub>,
        config:      TicConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            session_mgr,
            hub,
            config,
            running: AtomicBool::new(false),
        })
    }

    /// Force a tick immediately, ignoring the running guard.
    /// Intended for manual triggering (e.g. via the `/api/tic/trigger` endpoint).
    pub async fn tick_now(self: Arc<Self>) {
        if let Err(e) = self.run_tick().await {
            warn!(error = %e, "TicManager: forced tick failed");
        }
    }

    /// Spawn the background timer.
    pub fn start(self: Arc<Self>, shutdown: tokio_util::sync::CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("TicManager started (interval={}s, batch={})", self.config.interval_secs, self.config.batch_size);
            let mut interval = tokio::time::interval(Duration::from_secs(self.config.interval_secs));
            // Skip missed ticks instead of bursting to catch up after a long tick.
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => { info!("TicManager: stopping"); break; }
                    _ = interval.tick() => { self.tick().await; }
                }
            }
        })
    }

    async fn tick(&self) {
        // Prevent concurrent ticks.
        if self.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            warn!("TicManager: previous tick still running, skipping");
            return;
        }

        let result = self.run_tick().await;
        self.running.store(false, Ordering::SeqCst);

        if let Err(e) = result {
            warn!(error = %e, "TicManager: tick failed");
        }
    }

    async fn run_tick(&self) -> anyhow::Result<()> {
        // 1. Fetch the oldest N unprocessed events.
        let events = mcp_events::pending_limited(&self.db, self.config.batch_size).await?;
        if events.is_empty() {
            return Ok(());
        }

        info!(count = events.len(), "TicManager: processing event batch");

        // 2. Mark as processed BEFORE running the agent — avoids double-processing
        //    if the process crashes mid-turn.
        let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
        mcp_events::mark_processed(&self.db, &ids).await?;

        // 3. Serialize events into the agent prompt.
        let prompt = build_prompt(&events);

        // 4. Create a fresh ephemeral session (agent_id = "tic", source = "tic").
        //    We bypass ChatHub entirely — TIC is not a user-facing source and should
        //    not appear in the sources table or consume a broadcast channel.
        let (session_id, _) = self.session_mgr.create_session(TIC_AGENT, TIC_SOURCE, false, true).await?;
        let handler = self.session_mgr.get_or_create_handler(session_id).await?;
        handler.set_auto_deny_approvals();

        // 5. Sink for session events — nobody subscribes; drop the receiver immediately
        //    so the channel is drained without buffering.
        let (tx, _rx) = mpsc::channel(32);
        let notify = crate::core::tools::notify::make_tool(Arc::clone(&self.hub), "TIC");

        handler.handle_message(&prompt, None, None, None, None, vec![notify], tx, true).await?;

        info!(session_id, count = events.len(), "TicManager: tick complete");
        Ok(())
    }
}

// ── Prompt builder ─────────────────────────────────────────────────────────────

fn build_prompt(events: &[crate::core::db::mcp_events::McpEvent]) -> String {
    use std::fmt::Write;

    let n = events.len();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let mut out = format!("[TIC] {n} pending event(s) — {now}\n");

    for (i, ev) in events.iter().enumerate() {
        let _ = write!(
            out,
            "\n=== Event {}/{n} ===\nSource:   {}\nType:     {}\nReceived: {}\nPayload:\n{}\n",
            i + 1,
            ev.source,
            ev.method,
            ev.created_at,
            indent_payload(&ev.payload),
        );
    }

    out
}

/// Pretty-print a JSON payload with 2-space indent, falling back to raw string.
fn indent_payload(payload: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
        if let Ok(pretty) = serde_json::to_string_pretty(&v) {
            return pretty.lines().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n");
        }
    }
    format!("  {payload}")
}
