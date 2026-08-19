//! Kernel-neutral interaction state and blocking-input ownership.

use std::time::Instant;

use crate::{
    action::AgentRequest, config::Config, input::InputState, input_page::InputPageSession,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    pub follow: bool,
    pub offset: usize,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            follow: true,
            offset: 0,
        }
    }
}

/// State whose lifetime follows local user interaction rather than a wire
/// message family. It is the only production owner for composer, focus,
/// blocking pages, queue, and legacy copy-navigation state.
pub struct InteractionModel {
    pub input: InputState,
    pub scroll: ScrollState,
    pub input_page: Option<InputPageSession>,
    pub help_visible: bool,
    pub approval: Option<ApprovalCard>,
    pub question: Option<String>,
    pub queue: Vec<String>,
    pub copy_toast: Option<(String, Instant)>,
}

impl InteractionModel {
    pub fn new(config: &Config) -> Self {
        Self {
            input: InputState::new(config),
            scroll: ScrollState::default(),
            input_page: None,
            help_visible: false,
            approval: None,
            question: None,
            queue: Vec::new(),
            copy_toast: None,
        }
    }
}

impl Default for InteractionModel {
    fn default() -> Self {
        Self::new(&Config::default())
    }
}
