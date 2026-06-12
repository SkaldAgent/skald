use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::core::db::{chat_llm_tools, chat_sessions_stack, stack_mcp_grants};
use crate::core::events::ServerEvent;

use super::{ChatSessionHandler, TurnOutcome};
use super::interface_tools::AgentRunConfig;

impl ChatSessionHandler {
    /// Lifecycle driver for an asynchronously-dispatched sub-agent frame.
    ///
    /// Called from a `tokio::spawn` task created by `dispatch_sub_agent`.
    /// Acquires the processing mutex independently from the parent turn (which
    /// has already returned and released it), so parent and child never hold
    /// the lock at the same time.
    ///
    /// On completion the child:
    ///   1. Emits `AgentDone`
    ///   2. Completes (or fails) the parent tool call
    ///   3. Terminates the child stack frame
    ///   4. Calls `resume_turn` so the parent continues from where it left off
    pub(super) async fn run_child_frame(
        self: Arc<Self>,
        child_stack_id:      i64,
        parent_tool_call_id: i64,
        parent_agent_id:     String,
        child_config:        AgentRunConfig,
        tx:                  mpsc::Sender<ServerEvent>,
    ) {
        let _guard = self.processing.lock().await;
        use std::sync::atomic::Ordering;
        self.cancelled.store(false, Ordering::Relaxed);

        let pool           = &self.db;
        let child_agent_id = child_config.agent_id.clone();

        info!(
            session_id  = self.session_id,
            child_stack = child_stack_id,
            agent       = %child_agent_id,
            "run_child_frame: started"
        );

        let _ = self.resume_pending_tools(child_stack_id, &child_config, &tx).await;
        let outcome = self.run_agent_turn(child_stack_id, &child_config, &tx).await;

        if let Err(e) = stack_mcp_grants::delete_for_stack(pool, child_stack_id).await {
            warn!(stack_id = child_stack_id, error = %e, "run_child_frame: failed to delete stack MCP grants");
        }

        match outcome {
            Ok(TurnOutcome::WaitingChild { child_stack_id: grandchild_id }) => {
                info!(
                    session_id = self.session_id,
                    grandchild = grandchild_id,
                    "run_child_frame: grandchild dispatched asynchronously — deferring"
                );
                return;
            }

            Ok(TurnOutcome::Final { content, .. }) => {
                let result_preview = if content.len() > 500 {
                    format!("{}…", &content[..500])
                } else {
                    content.clone()
                };
                tx.send(ServerEvent::AgentDone {
                    stack_id:        child_stack_id,
                    agent_id:        child_agent_id.clone(),
                    parent_agent_id: parent_agent_id.clone(),
                    result_preview,
                }).await.ok();
                if let Err(e) = chat_llm_tools::complete(pool, parent_tool_call_id, &content).await {
                    error!(error = %e, tool_call_id = parent_tool_call_id, "run_child_frame: failed to complete parent tool call");
                }
                tx.send(ServerEvent::ToolDone {
                    tool_call_id: parent_tool_call_id,
                    result:       content,
                }).await.ok();
            }

            Ok(TurnOutcome::Cancelled) => {
                let msg = format!("Sub-agent `{child_agent_id}` was cancelled.");
                warn!(session_id = self.session_id, child_stack = child_stack_id, "run_child_frame: cancelled");
                tx.send(ServerEvent::AgentDone {
                    stack_id:        child_stack_id,
                    agent_id:        child_agent_id,
                    parent_agent_id: parent_agent_id,
                    result_preview:  "⚠️ Cancelled.".to_string(),
                }).await.ok();
                let _ = chat_llm_tools::fail(pool, parent_tool_call_id, &msg).await;
                tx.send(ServerEvent::ToolError { tool_call_id: parent_tool_call_id, error: msg }).await.ok();
            }

            Ok(TurnOutcome::Exhausted) => {
                let msg = format!(
                    "Sub-agent `{child_agent_id}` exceeded {} tool-call rounds without producing a final answer.",
                    self.max_tool_rounds
                );
                error!(session_id = self.session_id, child_stack = child_stack_id, "run_child_frame: exhausted");
                tx.send(ServerEvent::AgentDone {
                    stack_id:        child_stack_id,
                    agent_id:        child_agent_id,
                    parent_agent_id: parent_agent_id,
                    result_preview:  "⚠️ Exhausted tool-call rounds.".to_string(),
                }).await.ok();
                let _ = chat_llm_tools::fail(pool, parent_tool_call_id, &msg).await;
                tx.send(ServerEvent::ToolError { tool_call_id: parent_tool_call_id, error: msg }).await.ok();
            }

            Err(e) => {
                let msg = e.to_string();
                error!(session_id = self.session_id, child_stack = child_stack_id, error = %msg, "run_child_frame: error");
                tx.send(ServerEvent::AgentDone {
                    stack_id:        child_stack_id,
                    agent_id:        child_agent_id,
                    parent_agent_id: parent_agent_id,
                    result_preview:  format!("⚠️ Error: {msg}"),
                }).await.ok();
                let _ = chat_llm_tools::fail(pool, parent_tool_call_id, &msg).await;
                tx.send(ServerEvent::ToolError { tool_call_id: parent_tool_call_id, error: msg }).await.ok();
            }
        }

        let _ = chat_sessions_stack::terminate(pool, child_stack_id).await;

        drop(_guard);

        info!(
            session_id  = self.session_id,
            child_stack = child_stack_id,
            parent_tc   = parent_tool_call_id,
            "run_child_frame: child done, resuming parent"
        );

        if let Err(e) = self.resume_turn(None, None, vec![], tx).await {
            error!(session_id = self.session_id, error = %e, "run_child_frame: resume_turn failed");
        }
    }
}
