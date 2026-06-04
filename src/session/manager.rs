use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::approval::ApprovalManager;
use crate::chat_event_bus::ChatEventBus;
use crate::clarification::ClarificationManager;
use crate::compactor::ContextCompactor;
use crate::config::DatetimeConfig;
use crate::db::{chat_sessions, chat_sessions_stack};
use crate::llm::LlmManager;
use crate::mcp::McpManager;
use crate::image_generate::ImageGeneratorManager;
use crate::memory::MemoryManager;
use crate::tools::ToolRegistry;

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
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn llm_manager(&self) -> Arc<LlmManager> {
        Arc::clone(&self.llm_manager)
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
        ));

        handler.weak_self.set(Arc::downgrade(&handler)).ok();
        self.active.lock().await.insert(session_id, handler.clone());
        Ok(handler)
    }
}
