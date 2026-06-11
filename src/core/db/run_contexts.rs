use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContextRow {
    pub id:            String,
    pub name:          String,
    pub description:   Option<String>,
    pub tool_group_id: Option<String>,
    pub created_at:    String,
}

type RawRow = (String, String, Option<String>, Option<String>, String);

fn from_raw((id, name, description, tool_group_id, created_at): RawRow) -> RunContextRow {
    RunContextRow { id, name, description, tool_group_id, created_at }
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<RunContextRow>> {
    let rows = sqlx::query_as::<_, RawRow>(
        "SELECT id, name, description, tool_group_id, created_at
         FROM   run_contexts
         ORDER  BY created_at ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(from_raw).collect())
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<RunContextRow>> {
    let row = sqlx::query_as::<_, RawRow>(
        "SELECT id, name, description, tool_group_id, created_at
         FROM   run_contexts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(from_raw))
}

pub async fn insert(
    pool:          &SqlitePool,
    id:            &str,
    name:          &str,
    description:   Option<&str>,
    tool_group_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO run_contexts (id, name, description, tool_group_id) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(tool_group_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_or_ignore(
    pool:          &SqlitePool,
    id:            &str,
    name:          &str,
    description:   Option<&str>,
    tool_group_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO run_contexts (id, name, description, tool_group_id) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(tool_group_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update(
    pool:          &SqlitePool,
    id:            &str,
    name:          &str,
    description:   Option<&str>,
    tool_group_id: Option<&str>,
) -> Result<bool> {
    let rows = sqlx::query(
        "UPDATE run_contexts SET name = ?, description = ?, tool_group_id = ? WHERE id = ?",
    )
    .bind(name)
    .bind(description)
    .bind(tool_group_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rows > 0)
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let rows = sqlx::query("DELETE FROM run_contexts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows > 0)
}

pub async fn set_run_context_for_session(
    pool:           &SqlitePool,
    session_id:     i64,
    run_context_id: Option<&str>,
) -> Result<()> {
    sqlx::query("UPDATE chat_sessions SET run_context_id = ? WHERE id = ?")
        .bind(run_context_id)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}
