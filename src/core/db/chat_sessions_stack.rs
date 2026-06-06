use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct SessionStack {
    pub id:                  i64,
    pub agent_id:            String,
    pub depth:               i64,
    pub parent_tool_call_id: Option<i64>,
}

pub async fn create(
    pool:                &SqlitePool,
    session_id:          i64,
    agent_id:            &str,
    agent_prompt:        Option<&str>,
    depth:               i64,
    parent_tool_call_id: Option<i64>,
) -> anyhow::Result<SessionStack> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO chat_sessions_stack (session_id, agent_id, agent_prompt, depth, parent_tool_call_id)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(session_id)
    .bind(agent_id)
    .bind(agent_prompt)
    .bind(depth)
    .bind(parent_tool_call_id)
    .fetch_one(pool)
    .await?;

    Ok(SessionStack { id, agent_id: agent_id.to_string(), depth, parent_tool_call_id })
}

/// Returns the deepest active (non-terminated) frame for a session.
pub async fn active_for_session(
    pool:       &SqlitePool,
    session_id: i64,
) -> anyhow::Result<Option<SessionStack>> {
    let row = sqlx::query_as::<_, (i64, String, i64, Option<i64>)>(
        "SELECT id, agent_id, depth, parent_tool_call_id
         FROM   chat_sessions_stack
         WHERE  session_id    = ?
           AND  terminated_at IS NULL
         ORDER  BY depth DESC
         LIMIT  1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_stack))
}

/// Returns the root (depth=0) stack frame for a session.
pub async fn main_for_session(
    pool:       &SqlitePool,
    session_id: i64,
) -> anyhow::Result<Option<SessionStack>> {
    let row = sqlx::query_as::<_, (i64, String, i64, Option<i64>)>(
        "SELECT id, agent_id, depth, parent_tool_call_id
         FROM   chat_sessions_stack
         WHERE  session_id = ? AND depth = 0
         ORDER  BY id ASC
         LIMIT  1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_stack))
}

/// Returns all stack frames for a session (including terminated), ordered by id ASC.
/// Used to reconstruct the full agent call tree from history.
pub async fn all_for_session(
    pool:       &SqlitePool,
    session_id: i64,
) -> anyhow::Result<Vec<SessionStack>> {
    let rows = sqlx::query_as::<_, (i64, String, i64, Option<i64>)>(
        "SELECT id, agent_id, depth, parent_tool_call_id
         FROM   chat_sessions_stack
         WHERE  session_id = ?
         ORDER  BY id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_stack).collect())
}

pub async fn find_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<SessionStack>> {
    let row = sqlx::query_as::<_, (i64, String, i64, Option<i64>)>(
        "SELECT id, agent_id, depth, parent_tool_call_id
         FROM   chat_sessions_stack
         WHERE  id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_stack))
}

/// Marks a stack frame as terminated (agent completed or was cancelled).
pub async fn terminate(pool: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE chat_sessions_stack SET terminated_at = datetime('now') WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

fn row_to_stack(
    (id, agent_id, depth, parent_tool_call_id): (i64, String, i64, Option<i64>),
) -> SessionStack {
    SessionStack { id, agent_id, depth, parent_tool_call_id }
}
