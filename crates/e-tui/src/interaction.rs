//! Kernel-neutral blocking interaction state.

use crate::action::AgentRequest;

/// One pending approval prompt owned by the frontend interaction lifecycle.
#[derive(Debug, Clone)]
pub struct ApprovalCard {
    pub id: String,
    pub tool_name: String,
    pub reason: String,
}

impl ApprovalCard {
    pub fn answer(self, allow: bool) -> AgentRequest {
        AgentRequest::ApprovalAnswer { id: self.id, allow }
    }
}
