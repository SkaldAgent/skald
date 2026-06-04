use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::approval::NewApprovalRule;
use crate::server::AppState;

use super::ApiError;

// ── GET /api/approval/rules ───────────────────────────────────────────────────

pub async fn list_rules(
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let rules = state.approval.list_rules().await?;
    Ok(Json(json!(rules)))
}

// ── POST /api/approval/rules ──────────────────────────────────────────────────

pub async fn create_rule(
    State(state): State<AppState>,
    Json(body): Json<NewApprovalRule>,
) -> Result<Json<Value>, ApiError> {
    let id = state.approval.add_rule(body).await?;
    Ok(Json(json!({ "id": id })))
}

// ── PUT /api/approval/rules/:id ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RulePath { pub id: i64 }

pub async fn update_rule(
    State(state): State<AppState>,
    Path(p): Path<RulePath>,
    Json(body): Json<NewApprovalRule>,
) -> Result<Json<Value>, ApiError> {
    state.approval.update_rule(p.id, body).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── DELETE /api/approval/rules/:id ────────────────────────────────────────────

pub async fn delete_rule(
    State(state): State<AppState>,
    Path(p): Path<RulePath>,
) -> Result<Json<Value>, ApiError> {
    state.approval.delete_rule(p.id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── POST /api/approval/pending/:request_id/resolve ───────────────────────────
//
// Resolve a pending approval by request_id, regardless of which session or
// source it belongs to.  Useful for Telegram sub-agent approvals when the
// Telegram keyboard is unavailable.

#[derive(Deserialize)]
pub struct ResolvePath { pub request_id: i64 }

#[derive(Deserialize)]
pub struct ResolveBody {
    /// "approve" (default) or "reject".
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub note: String,
}

fn default_action() -> String { "approve".to_string() }

pub async fn resolve_pending(
    State(state): State<AppState>,
    Path(p): Path<ResolvePath>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Value>, ApiError> {
    if body.action == "reject" {
        let note = if body.note.is_empty() { "Rejected via API.".to_string() } else { body.note.clone() };
        state.chat_hub.reject(p.request_id, note).await;
    } else {
        state.chat_hub.approve(p.request_id).await;
    }
    Ok(Json(json!({ "ok": true, "request_id": p.request_id, "action": body.action })))
}

// ── GET /api/approval/pending ─────────────────────────────────────────────────
//
// Returns all currently-pending approval requests (all sessions).

pub async fn list_pending(
    State(state): State<AppState>,
) -> Json<Value> {
    let pending = state.approval.list_pending().await;
    Json(json!(pending))
}

// ── GET /api/approval/tools ───────────────────────────────────────────────────
//
// Returns all available tools (built-in + MCP) so the frontend can show a
// picker with names and descriptions when creating approval rules.

pub async fn list_tools(
    State(state): State<AppState>,
) -> Json<Value> {
    // Built-in tools
    let mut built_in: Vec<Value> = state.tools.list_all().iter().map(|(name, desc)| {
        json!({
            "name":        name,
            "description": desc,
            "source":      "built-in",
            "server":      null,
        })
    }).collect();

    // Synthesised tools visible at root level (call_agent, update_scratchpad, ask_user_clarification)
    let synthetic = [
        ("call_agent",               "Delegate a task to a specialised sub-agent."),
        ("update_scratchpad",        "Write a key-value note into the session scratchpad."),
        ("ask_user_clarification",   "Pause and ask the user a clarification question."),
    ];
    for (name, desc) in synthetic {
        built_in.push(json!({
            "name":        name,
            "description": desc,
            "source":      "built-in",
            "server":      null,
        }));
    }
    built_in.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    // MCP tools — group by server so the UI can show the server name.
    // t.name is the raw tool name ("send_message"); the full id used by the
    // approval gate is t.tool_id() → "mcp__gmail__send_message".
    let mcp_tools: Vec<Value> = state.mcp.tools().iter().map(|t| {
        json!({
            "name":        t.tool_id(),
            "description": t.description,
            "source":      "mcp",
            "server":      t.server_name,
        })
    }).collect();

    Json(json!({
        "built_in": built_in,
        "mcp":      mcp_tools,
    }))
}
