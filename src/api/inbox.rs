use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::server::AppState;
use super::ApiError;

// ── GET /api/inbox ────────────────────────────────────────────────────────────
//
// Returns all pending approval requests and clarification requests in a single
// response, so the frontend can show a unified Agent Inbox page with one fetch.

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    let approvals      = state.approval.list_pending().await;
    let clarifications = state.clarification.list_pending().await;
    let total          = approvals.len() + clarifications.len();
    Json(json!({
        "total":          total,
        "approvals":      approvals,
        "clarifications": clarifications,
    }))
}

// ── POST /api/inbox/approvals/:request_id/resolve ─────────────────────────────

#[derive(Deserialize)]
pub struct ApprovePath { pub request_id: i64 }

#[derive(Deserialize)]
pub struct ApproveBody {
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub note: String,
}

fn default_action() -> String { "approve".to_string() }

pub async fn resolve_approval(
    State(state): State<AppState>,
    Path(p):      Path<ApprovePath>,
    Json(body):   Json<ApproveBody>,
) -> Result<Json<Value>, ApiError> {
    if body.action == "reject" {
        let note = if body.note.is_empty() { "Rejected via Agent Inbox.".into() } else { body.note.clone() };
        state.chat_hub.reject(p.request_id, note).await;
    } else {
        state.chat_hub.approve(p.request_id).await;
    }
    Ok(Json(json!({ "ok": true, "request_id": p.request_id, "action": body.action })))
}

// ── POST /api/inbox/clarifications/:request_id/resolve ────────────────────────

#[derive(Deserialize)]
pub struct ClarifyPath { pub request_id: i64 }

#[derive(Deserialize)]
pub struct ClarifyBody {
    pub answer: String,
}

pub async fn resolve_clarification(
    State(state): State<AppState>,
    Path(p):      Path<ClarifyPath>,
    Json(body):   Json<ClarifyBody>,
) -> Result<Json<Value>, ApiError> {
    if body.answer.trim().is_empty() {
        return Err(ApiError::bad_request("answer must not be empty"));
    }
    let resolved = state.clarification.resolve(p.request_id, body.answer).await;
    if resolved {
        Ok(Json(json!({ "ok": true, "request_id": p.request_id })))
    } else {
        Err(ApiError::not_found("clarification request not found"))
    }
}
