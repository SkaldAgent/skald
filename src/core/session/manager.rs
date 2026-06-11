use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::core::agents;
use crate::core::approval::ApprovalManager;
use crate::core::chat_event_bus::ChatEventBus;
use crate::core::clarification::ClarificationManager;
use crate::core::compactor::ContextCompactor;
use crate::core::config::DatetimeConfig;
use crate::core::db::{chat_sessions, chat_sessions_stack};
use crate::core::llm::LlmManager;
use crate::core::mcp::McpManager;
use crate::core::image_generate::ImageGeneratorManager;
use crate::core::memory::MemoryManager;
use crate::core::run_context::RunContextManager;
use crate::core::tools::ToolRegistry;

use super::handler::ChatSessionHandler;

pub struct ChatSessionManager {
    db:                    Arc<SqlitePool>,
    llm_manager:           Arc<LlmManager>,
    max_history_messages:  usize,
    max_tool_rounds:       usize,
    max_tool_result_chars: Option<usize>,
    datetime_config:       DatetimeConfig,
    tools:                 Arc<ToolRegistry>,
    mcp:                   Arc<McpManager>,
    approval:              Arc<ApprovalManager>,
    clarification:         Arc<ClarificationManager>,
    event_bus:             Arc<ChatEventBus>,
    memory_manager:          Arc<MemoryManager>,
    image_generator_manager: Arc<ImageGeneratorManager>,
    /// Shared compactor instance, `None` when compaction is disabled.
    compactor:               Option<Arc<ContextCompactor>>,
    run_context_manager:     Arc<RunContextManager>,
    active:                Mutex<HashMap<i64, Arc<ChatSessionHandler>>>,
}

impl ChatSessionManager {
    pub fn new(
        db:                    Arc<SqlitePool>,
        llm_manager:           Arc<LlmManager>,
        max_history_messages:  usize,
        max_tool_rounds:       usize,
        max_tool_result_chars: Option<usize>,
        datetime_config:       DatetimeConfig,
        tools:                 Arc<ToolRegistry>,
        mcp:                   Arc<McpManager>,
        approval:              Arc<ApprovalManager>,
        clarification:         Arc<ClarificationManager>,
        event_bus:             Arc<ChatEventBus>,
        memory_manager:          Arc<MemoryManager>,
        image_generator_manager: Arc<ImageGeneratorManager>,
        compactor:               Option<Arc<ContextCompactor>>,
        run_context_manager:     Arc<RunContextManager>,
    ) -> Self {
        Self {
            db,
            llm_manager,
            max_history_messages,
            max_tool_rounds,
            max_tool_result_chars,
            datetime_config,
            tools,
            mcp,
            approval,
            clarification,
            event_bus,
            memory_manager,
            image_generator_manager,
            compactor,
            run_context_manager,
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn llm_manager(&self) -> Arc<LlmManager> {
        Arc::clone(&self.llm_manager)
    }

    pub fn run_context_manager(&self) -> Arc<RunContextManager> {
        Arc::clone(&self.run_context_manager)
    }

    /// Returns the live handler for `session_id` if it is currently loaded,
    /// without creating a new one. Used by the API for in-place updates.
    pub async fn active_handler(&self, session_id: i64) -> Option<Arc<ChatSessionHandler>> {
        self.active.lock().await.get(&session_id).cloned()
    }

    pub async fn create_session(
        &self,
        agent_id:       &str,
        source:         &str,
        is_interactive: bool,
        is_ephemeral:   bool,
    ) -> anyhow::Result<(i64, i64)> {
        let session = chat_sessions::create(&self.db, agent_id, source, is_interactive, is_ephemeral).await?;
        let stack   = chat_sessions_stack::create(
            &self.db, session.id, "main", None, 0, None,
        ).await?;
        Ok((session.id, stack.id))
    }

    pub async fn get_or_create_handler(
        &self,
        session_id: i64,
    ) -> anyhow::Result<Arc<ChatSessionHandler>> {
        {
            let active = self.active.lock().await;
            if let Some(h) = active.get(&session_id) {
                return Ok(h.clone());
            }
        }

        let session = chat_sessions::find_by_id(&self.db, session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("session {session_id} not found"))?;

        // Resolve run_context: session row > agent meta.json default > None (falls back to "default" implicitly)
        let run_context_id: Option<String> = session.run_context_id.clone().or_else(|| {
            agents::load_meta(&session.agent_id).ok()
                .and_then(|m| m.run_context)
        });
        let run_context = if let Some(ref rc_id) = run_context_id {
            self.run_context_manager.get_context(rc_id).await.unwrap_or(None)
        } else {
            None
        };

        let handler = Arc::new(ChatSessionHandler::new(
            session_id,
            self.db.clone(),
            Arc::clone(&self.llm_manager),
            self.max_history_messages,
            self.max_tool_rounds,
            self.max_tool_result_chars,
            self.datetime_config.clone(),
            session.agent_id,
            session.source,
            session.is_interactive,
            session.is_ephemeral,
            self.tools.clone(),
            self.mcp.clone(),
            Arc::clone(&self.approval),
            Arc::clone(&self.clarification),
            Arc::clone(&self.event_bus),
            Arc::clone(&self.memory_manager),
            Arc::clone(&self.image_generator_manager),
            self.compactor.clone(),
            run_context,
        ));

        handler.weak_self.set(Arc::downgrade(&handler)).ok();
        self.active.lock().await.insert(session_id, handler.clone());
        Ok(handler)
    }
}
