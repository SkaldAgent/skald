use anyhow::Result;
use sqlx::SqlitePool;

use crate::approval::{ApprovalRule, NewApprovalRule, RuleAction};

type RawRow = (i64, Option<String>, Option<String>, String, Option<String>, String, Option<String>, i64);

fn from_raw((id, agent_id, source, tool_pattern, path_pattern, action, note, priority): RawRow)
    -> anyhow::Result<ApprovalRule>
{
    let action: RuleAction = action.parse()?;
    Ok(ApprovalRule { id, agent_id, source, tool_pattern, path_pattern, action, note, priority })
}

/// Returns all rules ordered by priority ASC (lowest number = evaluated first).
pub async fn list(pool: &SqlitePool) -> Result<Vec<ApprovalRule>> {
    let rows = sqlx::query_as::<_, RawRow>(
        "SELECT id, agent_id, source, tool_pattern, path_pattern, action, note, priority
         FROM   approval_rules
         ORDER  BY priority ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(from_raw).collect()
}

/// Inserts a new rule; returns its id.
pub async fn insert(pool: &SqlitePool, r: NewApprovalRule) -> Result<i64> {
    let priority = r.priority.unwrap_or(100);
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO approval_rules (agent_id, source, tool_pattern, path_pattern, action, note, priority)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(r.agent_id)
    .bind(r.source)
    .bind(r.tool_pattern)
    .bind(r.path_pattern)
    .bind(r.action.as_str())
    .bind(r.note)
    .bind(priority)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Updates an existing rule by id.
pub async fn update(pool: &SqlitePool, id: i64, r: NewApprovalRule) -> Result<()> {
    let priority = r.priority.unwrap_or(100);
    sqlx::query(
        "UPDATE approval_rules
         SET agent_id = ?, source = ?, tool_pattern = ?, path_pattern = ?, action = ?, note = ?, priority = ?
         WHERE id = ?",
    )
    .bind(r.agent_id)
    .bind(r.source)
    .bind(r.tool_pattern)
    .bind(r.path_pattern)
    .bind(r.action.as_str())
    .bind(r.note)
    .bind(priority)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Deletes a rule by id.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM approval_rules WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
