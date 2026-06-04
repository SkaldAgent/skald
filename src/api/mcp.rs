use axum::Json;
use axum::extract::State;
use serde_json::Value;

use crate::server::AppState;

/// Returns the list of running MCP servers and their available tools.
pub async fn list_servers(State(state): State<AppState>) -> Json<Vec<Value>> {
    Json(state.mcp.server_infos())
}
