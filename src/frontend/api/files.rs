use std::path::Path;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use std::sync::Arc;
use crate::core::skald::Skald;
use crate::core::tools::fs as fs_tools;
use super::ApiError;

#[derive(Serialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
}

pub async fn list_files(State(_state): State<Arc<Skald>>) -> Result<Json<Vec<FileEntry>>, ApiError> {
    let root = fs_tools::resolve(".")?;
    let mut paths: Vec<String> = Vec::new();
    walk(&root, &root, &mut paths)?;
    paths.sort();

    let entries = paths
        .into_iter()
        .map(|p| {
            let name = Path::new(&p)
                .file_stem()
                .map_or_else(|| p.clone(), |s| s.to_string_lossy().to_string());
            FileEntry { path: p, name }
        })
        .collect();
    Ok(Json(entries))
}

#[derive(Deserialize)]
pub struct FileQuery {
    pub path: String,
}

pub async fn get_file(
    State(_state): State<Arc<Skald>>,
    Query(q): Query<FileQuery>,
) -> Result<String, ApiError> {
    let abs = fs_tools::resolve(&q.path)?;
    let content = std::fs::read_to_string(&abs)
        .map_err(|_| anyhow::anyhow!("File not found: {}", q.path))?;
    Ok(content)
}

#[derive(Deserialize)]
pub struct SavePayload {
    pub path:    String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct CreatePayload {
    pub path: String,
}

pub async fn create_file(
    State(_state): State<Arc<Skald>>,
    Json(body): Json<CreatePayload>,
) -> Result<StatusCode, ApiError> {
    let abs = fs_tools::resolve(&body.path)?;
    if abs.exists() {
        return Err(anyhow::anyhow!("File già esistente: {}", body.path).into());
    }
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&abs, "")?;
    Ok(StatusCode::CREATED)
}

pub async fn save_file(
    State(_state): State<Arc<Skald>>,
    Json(body): Json<SavePayload>,
) -> Result<StatusCode, ApiError> {
    let abs = fs_tools::resolve(&body.path)?;
    if !abs.exists() {
        return Err(anyhow::anyhow!("File not found: {}", body.path).into());
    }
    std::fs::write(&abs, &body.content)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RenamePayload {
    pub old_path: String,
    pub new_path: String,
}

pub async fn rename_file(
    State(_state): State<Arc<Skald>>,
    Json(body): Json<RenamePayload>,
) -> Result<StatusCode, ApiError> {
    let old_abs = fs_tools::resolve(&body.old_path)?;
    let new_abs = fs_tools::resolve(&body.new_path)?;
    if !old_abs.exists() {
        return Err(anyhow::anyhow!("File non trovato: {}", body.old_path).into());
    }
    if new_abs.exists() {
        return Err(anyhow::anyhow!("File già esistente: {}", body.new_path).into());
    }
    if let Some(parent) = new_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&old_abs, &new_abs)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_file(
    State(_state): State<Arc<Skald>>,
    Query(q): Query<FileQuery>,
) -> Result<StatusCode, ApiError> {
    let abs = fs_tools::resolve(&q.path)?;
    if !abs.exists() {
        return Err(anyhow::anyhow!("File non trovato: {}", q.path).into());
    }
    std::fs::remove_file(&abs)?;
    Ok(StatusCode::NO_CONTENT)
}

fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) -> anyhow::Result<()> {
    if !dir.exists() { return Ok(()); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if matches!(name, ".git" | "target" | "node_modules") { continue; }
            walk(root, &path, out)?;
        } else if path.is_file() {
            let rel = path.strip_prefix(root)?.to_string_lossy().to_string();
            out.push(rel);
        }
    }
    Ok(())
}
