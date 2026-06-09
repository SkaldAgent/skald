use std::sync::Arc;

use serde::Serialize;

use crate::core::approval::{ApprovalManager, PendingApprovalInfo};
use crate::core::clarification::{ClarificationManager, PendingClarificationInfo};

#[derive(Serialize)]
pub struct InboxItems {
    pub total:          usize,
    pub approvals:      Vec<PendingApprovalInfo>,
    pub clarifications: Vec<PendingClarificationInfo>,
}

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
