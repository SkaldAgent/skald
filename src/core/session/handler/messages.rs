use std::collections::HashSet;
use std::sync::Arc;

use serde_json::Value;

use super::ChatSessionHandler;
use super::message_builder::MessageBuilder;

impl ChatSessionHandler {
    /// Thin wrapper: constructs a `MessageBuilder` from this handler's fields
    /// and delegates to `MessageBuilder::build`.
    ///
    /// See `MessageBuilder::build` for the full documentation and message ordering.
    pub(super) async fn build_openai_messages(
        &self,
        pool:                 &sqlx::SqlitePool,
        stack_id:             i64,
        agent_id:             &str,
        extra_system_static:  Option<&str>,
        extra_system_dynamic: Option<&str>,
        tail_reminder:        Option<&str>,
        active_mcp_grants:    &HashSet<String>,
        cache_hints:          bool,
    ) -> anyhow::Result<Vec<Value>> {
        let builder = MessageBuilder {
            pool:                  Arc::clone(&self.db),
            session_id:            self.session_id,
            mcp:                   Arc::clone(&self.mcp),
            datetime_config:       self.datetime_config.clone(),
            max_history_messages:  self.max_history_messages,
            max_tool_result_chars: self.max_tool_result_chars,
            compactor:             self.compactor.clone(),
        };
        // `pool` is passed in from the caller (always `&self.db`) but we take
        // ownership via Arc::clone above so the signature stays backward-compatible.
        let _ = pool; // suppress unused-variable warning; MessageBuilder uses its own Arc
        builder.build(stack_id, agent_id, extra_system_static, extra_system_dynamic, tail_reminder, active_mcp_grants, cache_hints).await
    }
}
