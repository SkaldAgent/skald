use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use core_api::inbox::{InboxApi, InboxApprovalItem, InboxClarificationItem, InboxSnapshot};

use crate::core::approval::{ApprovalManager, PendingApprovalInfo};
use crate::core::clarification::{ClarificationManager, PendingClarificationInfo};

#[derive(Serialize)]
pub struct InboxItems {
    pub total:          usize,
    pub approvals:      Vec<PendingApprovalInfo>,
    pub clarifications: Vec<PendingClarificationInfo>,
}

#[derive(Clone)]
pub struct Inbox {
    pub approval:  Arc<ApprovalManager>,
    clarification: Arc<ClarificationManager>,
}

impl Inbox {
    pub fn new(
        approval:      Arc<ApprovalManager>,
        clarification: Arc<ClarificationManager>,
    ) -> Self {
        Self { approval, clarification }
    }

    pub async fn list_pending(&self) -> InboxItems {
        let approvals      = self.approval.list_pending().await;
        let clarifications = self.clarification.list_pending().await;
        let total          = approvals.len() + clarifications.len();
        InboxItems { total, approvals, clarifications }
    }

    pub async fn approve(&self, request_id: i64) {
        self.approval.approve(request_id).await;
    }

    pub async fn reject(&self, request_id: i64, note: String) {
        self.approval.reject(request_id, note).await;
    }

    pub async fn answer(&self, request_id: i64, answer: String) -> bool {
        self.clarification.resolve(request_id, answer).await
    }
}

/// Exposes the Inbox to plugins via `PluginContext` (plugin.md §12.2). Converts
/// the main-crate pending types into the core-api snapshot types.
#[async_trait]
impl InboxApi for Inbox {
    async fn list_pending(&self) -> InboxSnapshot {
        let items = self.list_pending().await;
        let approvals = items.approvals.into_iter().map(|a| InboxApprovalItem {
            request_id:    a.request_id,
            tool_name:     a.tool_name,
            agent_id:      a.agent_id,
            source:        a.source,
            context_label: a.context_label,
            created_at:    a.created_at,
        }).collect();
        let clarifications = items.clarifications.into_iter().map(|c| InboxClarificationItem {
            request_id:        c.request_id,
            agent_id:          c.agent_id,
            source:            c.source,
            context_label:     c.context_label,
            title:             c.title,
            question:          c.question,
            suggested_answers: c.suggested_answers,
            created_at:        c.created_at,
        }).collect();
        InboxSnapshot { total: items.total, approvals, clarifications }
    }

    async fn approve(&self, request_id: i64) {
        self.approve(request_id).await;
    }

    async fn reject(&self, request_id: i64, reason: String) {
        self.reject(request_id, reason).await;
    }

    async fn answer(&self, request_id: i64, answer: String) -> bool {
        self.answer(request_id, answer).await
    }
}
