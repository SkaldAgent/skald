use axum::{
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio::fs;

use crate::server::AppState;

/// GET /api/images/:task_id
///
/// Serves a generated image from `data/images/<task_id>.png`.
pub async fn get_image(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Response {
    // Reject any path traversal attempts.
    if task_id.contains('/') || task_id.contains('\\') || task_id.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let task_id = task_id.trim_end_matches(".png");
    let path = state.image_generator_manager.images_dir().join(format!("{task_id}.png"));

    match fs::read(&path).await {
        Ok(bytes) => {
            let mut response = bytes.into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("image/png"),
            );
            response
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
