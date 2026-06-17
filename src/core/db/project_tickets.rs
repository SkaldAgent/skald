use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProjectTicket {
    pub id:           i64,
    pub project_id:   i64,
    pub title:        String,
    pub description:  String,
    pub status:       String,
    pub agent_id:     String,
    pub run_context:  Option<String>,
    pub job_id:       Option<i64>,
    pub result:       Option<String>,
    pub error:        Option<String>,
    pub created_at:   String,
    pub started_at:   Option<String>,
    pub completed_at: Option<String>,
}

const SELECT: &str =
    "SELECT id, project_id, title, description, status, agent_id, run_context,
            job_id, result, error, created_at, started_at, completed_at
     FROM project_tickets";

pub async fn list_for_project(pool: &SqlitePool, project_id: i64) -> Result<Vec<ProjectTicket>> {
    let rows = sqlx::query_as::<_, ProjectTicket>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE project_id = ? ORDER BY id"
    )))
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<ProjectTicket>> {
    let row = sqlx::query_as::<_, ProjectTicket>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create(
    pool:        &SqlitePool,
    project_id:  i64,
    title:       &str,
    description: &str,
    agent_id:    &str,
    run_context: Option<&str>,
) -> Result<ProjectTicket> {
    let id = sqlx::query(
        "INSERT INTO project_tickets (project_id, title, description, agent_id, run_context)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(project_id)
    .bind(title)
    .bind(description)
    .bind(agent_id)
    .bind(run_context)
    .execute(pool)
    .await?
    .last_insert_rowid();

    let row = sqlx::query_as::<_, ProjectTicket>(sqlx::AssertSqlSafe(format!(
        "{SELECT} WHERE id = ?"
    )))
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool> {
    let n = sqlx::query("DELETE FROM project_tickets WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

pub async fn set_status(pool: &SqlitePool, id: i64, status: &str) -> Result<()> {
    sqlx::query("UPDATE project_tickets SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark as in_progress and record the scheduled job that is running it.
pub async fn start(pool: &SqlitePool, id: i64, job_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE project_tickets
         SET status = 'in_progress', job_id = ?, started_at = datetime('now')
         WHERE id = ?",
    )
    .bind(job_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mark as done or failed, recording result/error and timestamp.
pub async fn complete(
    pool:   &SqlitePool,
    id:     i64,
    result: Option<&str>,
    error:  Option<&str>,
) -> Result<()> {
    let status = if error.is_some() { "failed" } else { "done" };
    sqlx::query(
        "UPDATE project_tickets
         SET status = ?, result = ?, error = ?, completed_at = datetime('now')
         WHERE id = ?",
    )
    .bind(status)
    .bind(result)
    .bind(error)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Reset a ticket back to todo, clearing all run state.
pub async fn reset(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE project_tickets
         SET status = 'todo', job_id = NULL, result = NULL, error = NULL,
             started_at = NULL, completed_at = NULL
         WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
