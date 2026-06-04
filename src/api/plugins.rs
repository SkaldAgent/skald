use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::server::AppState;
use super::ApiError;

pub async fn list(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let plugins = state.plugin_manager.list().await?;
    Ok(Json(plugins))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    pub enabled: bool,
    pub config:  Value,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id):     Path<String>,
    Json(body):   Json<UpdateBody>,
) -> Result<impl IntoResponse, ApiError> {
    state.plugin_manager.update_config(&id, body.enabled, body.config).await?;
    Ok(())
}
