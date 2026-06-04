use sqlx::SqlitePool;

pub struct ChatSession {
    pub id:             i64,
    pub source:         String,
    pub agent_id:       String,
    /// True when a real user is actively participating (web, telegram).
    /// False for fully automated sessions (cron, tic).
    pub is_interactive: bool,
    /// True for short-lived task sessions (cron, tic) with no long-term
    /// conversational value. May be used to skip memory / analytics sinks.
    pub is_ephemeral:   bool,
}

pub async fn create(
    pool:           &SqlitePool,
    agent_id:       &str,
    source:         &str,
    is_interactive: bool,
    is_ephemeral:   bool,
) -> anyhow::Result<ChatSession> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO chat_sessions (source, agent_id, is_interactive, is_ephemeral)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(source)
    .bind(agent_id)
    .bind(is_interactive as i64)
    .bind(is_ephemeral as i64)
    .fetch_one(pool)
    .await?;

    Ok(ChatSession {
        id,
        source:         source.to_string(),
        agent_id:       agent_id.to_string(),
        is_interactive,
        is_ephemeral,
    })
}

pub async fn find_by_id(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<ChatSession>> {
    let row = sqlx::query_as::<_, (i64, String, String, bool, bool)>(
        "SELECT id, source, agent_id, is_interactive, is_ephemeral
         FROM chat_sessions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id, source, agent_id, is_interactive, is_ephemeral)| ChatSession {
        id,
        source,
        agent_id,
        is_interactive,
        is_ephemeral,
    }))
}
