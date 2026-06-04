use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::db::{chat_history, chat_llm_tools, chat_sessions_stack, sources};
use crate::db::chat_sessions_stack::SessionStack;
use crate::server::AppState;
use crate::session::handler::ApprovalDecision;
use crate::tools::{ToolRegistry, ToolDescriptionLength};

use super::ApiError;

// ── POST /api/sessions — start a new conversation ─────────────────────────────

#[derive(Deserialize)]
pub struct CreateQuery {
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String { "web".to_string() }

pub async fn create(
    State(state): State<AppState>,
    Query(q): Query<CreateQuery>,
) -> Result<Json<Value>, ApiError> {
    state.chat_hub.clear(&q.source).await?;
    Ok(Json(json!({})))
}

// ── GET /api/web/messages ─────────────────────────────────────────────────────

pub async fn web_messages(
    State(state): State<AppState>,
) -> Result<Json<Vec<Value>>, ApiError> {
    messages_for_source(&state, "web").await
}

// ── GET /api/:source/messages ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SourcePath { pub source: String }

pub async fn source_messages(
    State(state): State<AppState>,
    Path(p): Path<SourcePath>,
) -> Result<Json<Vec<Value>>, ApiError> {
    messages_for_source(&state, &p.source).await
}

async fn messages_for_source(state: &AppState, source: &str) -> Result<Json<Vec<Value>>, ApiError> {
    let session_id = match sources::active_session_id(&state.db, source).await? {
        Some(id) => id,
        None     => return Ok(Json(vec![])),
    };

    let main_stack = match chat_sessions_stack::main_for_session(&state.db, session_id).await? {
        Some(s) => s,
        None    => return Ok(Json(vec![])),
    };

    let subagent_map: HashMap<i64, SessionStack> =
        chat_sessions_stack::all_for_session(&state.db, session_id)
            .await?
            .into_iter()
            .filter_map(|s| s.parent_tool_call_id.map(|tc_id| (tc_id, s)))
            .collect();

    let mut items: Vec<Value> = Vec::new();
    build_items(&state.db, &state.tools, &main_stack, &subagent_map, &mut items).await?;

    Ok(Json(items))
}

// ── POST /api/web/tools/:tool_call_id/resolve — approve/reject pending tool ───

#[derive(Deserialize)]
pub struct ResolveToolPath {
    pub tool_call_id: i64,
}

#[derive(Deserialize)]
pub struct ResolveToolBody {
    /// `"approve"` or `"reject"`
    pub action: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize)]
pub struct ResolveToolResponse {
    pub tool_call_id: i64,
    pub status:       String,
    pub result:       Option<String>,
}

/// Approve or reject a `pending` tool call from an interrupted session.
/// Session is resolved automatically from the "web" source's active session.
pub async fn web_resolve_tool(
    State(state): State<AppState>,
    Path(p): Path<ResolveToolPath>,
    Json(body): Json<ResolveToolBody>,
) -> Result<Json<ResolveToolResponse>, ApiError> {
    let session_id = sources::active_session_id(&state.db, "web")
        .await?
        .ok_or_else(|| anyhow::anyhow!("no active web session"))?;

    let tc = sqlx::query_as::<_, (i64, String, Option<String>, String)>(
        "SELECT t.id, t.name, t.arguments, t.status
         FROM   chat_llm_tools t
         JOIN   chat_history h ON h.id = t.message_id
         JOIN   chat_sessions_stack ss ON ss.id = h.session_stack_id
         WHERE  t.id = ? AND ss.session_id = ?",
    )
    .bind(p.tool_call_id)
    .bind(session_id)
    .fetch_optional(&*state.db)
    .await?
    .ok_or_else(|| anyhow::anyhow!(
        "tool_call_id {} not found in current web session", p.tool_call_id
    ))?;

    let (tc_id, tc_name, tc_args_raw, tc_status) = tc;

    if tc_status != "pending" {
        return Err(anyhow::anyhow!(
            "tool_call {} is not pending (status: {})", tc_id, tc_status
        ).into());
    }

    let args: Value = tc_args_raw.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Object(Default::default()));

    if body.action == "reject" {
        let note = if body.note.is_empty() {
            "Rejected via API.".to_string()
        } else {
            format!("Rejected via API: {}", body.note)
        };
        let live = state.approval
            .resolve_for_tool_call(tc_id, ApprovalDecision::Rejected { note: note.clone() })
            .await;
        if !live {
            chat_llm_tools::fail(&state.db, tc_id, &note).await?;
        }
        return Ok(Json(ResolveToolResponse {
            tool_call_id: tc_id,
            status:       "failed".to_string(),
            result:       Some(note),
        }));
    }

    // `restart` calls process::exit — mark done in DB first.
    if tc_name == "restart" {
        chat_llm_tools::complete(&state.db, tc_id, "Riavvio avviato.").await?;
        std::process::exit(-1);
    }

    // ── Live path: LLM loop is blocked waiting for approval ──────────────────
    if state.approval
        .resolve_for_tool_call(tc_id, ApprovalDecision::Approved)
        .await
    {
        return Ok(Json(ResolveToolResponse {
            tool_call_id: tc_id,
            status:       "running".to_string(),
            result:       None,
        }));
    }

    // ── Post-restart path: no loop in memory, execute directly ───────────────
    let handler = state.chat_hub.session_handler("web").await?;
    match handler.execute_tool(&tc_name, args).await {
        Ok(result) => {
            chat_llm_tools::complete(&state.db, tc_id, &result).await?;
            Ok(Json(ResolveToolResponse {
                tool_call_id: tc_id,
                status:       "done".to_string(),
                result:       Some(result),
            }))
        }
        Err(e) => {
            let msg = e.to_string();
            chat_llm_tools::fail(&state.db, tc_id, &msg).await?;
            Err(anyhow::anyhow!(msg).into())
        }
    }
}

// ── Recursive message-tree builder ────────────────────────────────────────────

fn build_items<'a>(
    db:           &'a SqlitePool,
    tools:        &'a ToolRegistry,
    stack:        &'a SessionStack,
    subagent_map: &'a HashMap<i64, SessionStack>,
    items:        &'a mut Vec<Value>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let messages = chat_history::for_stack_all(db, stack.id).await?;

        for msg in &messages {
            let failed = msg.status == "failed";
            match msg.role {
                chat_history::Role::User => {
                    // Skip synthetic messages (TIC notifications, etc.) — they are
                    // injected as user turns for the LLM but must not appear in the UI.
                    if msg.is_synthetic {
                        continue;
                    }
                    items.push(json!({ "kind": "user", "content": msg.content, "failed": failed }));
                }
                chat_history::Role::Agent => {}
                chat_history::Role::Assistant => {
                    let tool_calls = chat_llm_tools::for_message(db, msg.id).await?;
                    if tool_calls.is_empty() {
                        items.push(json!({
                            "kind":          "assistant",
                            "content":       msg.content,
                            "failed":        failed,
                            "input_tokens":  msg.input_tokens,
                            "output_tokens": msg.output_tokens,
                        }));
                    } else {
                        if !msg.content.trim().is_empty() {
                            items.push(json!({
                                "kind":          "thinking",
                                "message_id":    msg.id,
                                "content":       msg.content,
                                "failed":        failed,
                                "input_tokens":  msg.input_tokens,
                                "output_tokens": msg.output_tokens,
                            }));
                        }
                        for tc in &tool_calls {
                            let args: Value = tc.arguments.as_deref()
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(Value::Null);

                            let (status, result, error) = match tc.status.as_str() {
                                "done"    => ("done",    tc.result.clone(), None),
                                // 'pending' means waiting for explicit user input (approval or
                                // clarification) — show the approval form with no error message.
                                "pending" => ("pending", None,              None),
                                // 'running' means the tool was mid-execution when the session was
                                // interrupted — shown as "Interrupted" so the frontend can auto-resume.
                                "running" => ("error",   None,              Some("Interrupted.".to_string())),
                                // 'failed' means the tool completed with a genuine error — show
                                // the actual error message, NOT "Interrupted" (that would trigger
                                // a spurious auto-resume on page refresh).
                                _         => ("error",   None,              tc.result.clone()),
                            };

                            let label_short = tools.describe_call(&tc.name, &args, ToolDescriptionLength::Short);
                            let label_full  = tools.describe_call(&tc.name, &args, ToolDescriptionLength::Full);
                            items.push(json!({
                                "kind":         "tool",
                                "tool_call_id": tc.id,
                                "name":         tc.name,
                                "label_short":  label_short,
                                "label_full":   label_full,
                                "arguments":    args,
                                "status":       status,
                                "result":       result,
                                "error":        error,
                            }));

                            if let Some(sub_stack) = subagent_map.get(&tc.id) {
                                items.push(json!({
                                    "kind":     "agent",
                                    "stack_id": sub_stack.id,
                                    "agent_id": sub_stack.agent_id,
                                    "depth":    sub_stack.depth,
                                    "done":     true,
                                }));
                                build_items(db, tools, sub_stack, subagent_map, items).await?;
                                items.push(json!({
                                    "kind":     "agent_end",
                                    "agent_id": sub_stack.agent_id,
                                    "depth":    sub_stack.depth,
                                }));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })
}
