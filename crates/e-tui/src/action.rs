//! Owned requests returned by synchronous frontend updates.

use std::time::Instant;

use crate::{
    preview::{PreviewContent, PreviewKey, PreviewRequest, PreviewRequestId, PreviewRevision},
    Config, ThemeFile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionAnswer {
    pub id: String,
    pub selected: Vec<String>,
    pub custom: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRequest {
    Input {
        text: String,
    },
    NewInput {
        mode: String,
        text: String,
    },
    Command {
        line: String,
    },
    Interrupt,
    Attach {
        session_id: String,
    },
    ListSessions,
    ApprovalAnswer {
        id: String,
        allow: bool,
    },
    AnswerQuestions {
        request_id: String,
        answers: Vec<QuestionAnswer>,
    },
    CancelQuestions {
        request_id: String,
    },
    History {
        before_sequence: u64,
        limit: usize,
    },
    LoginGet,
    LoginSetApiKey {
        provider: String,
        value: String,
    },
    LoginProxyCreate {
        base_url: String,
        api_key: String,
        protocol: String,
        model: String,
    },
    LoginProxyDelete {
        id: String,
    },
    ModelGet,
    ModelSet {
        provider: String,
        model: String,
    },
    Ping,
}

#[derive(Debug, Clone)]
pub enum EffectResult {
    ConfigPersisted(Result<(), String>),
    ConfigReloaded {
        config: Box<Config>,
        themes: Vec<ThemeFile>,
    },
    ConfigReloadFailed(String),
    ClipboardWritten {
        lines: usize,
    },
    ClipboardFailed(String),
    PreviewResolved {
        request_id: PreviewRequestId,
        key: PreviewKey,
        revision: PreviewRevision,
        result: Result<PreviewContent, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawPriority {
    Interactive,
    Content,
    Animation,
}

#[derive(Debug, Clone)]
pub enum UiAction {
    Agent(AgentRequest),
    ResolvePreview(PreviewRequest),
    PersistConfig(Config),
    ReloadConfig,
    PersistSessionId(String),
    WriteClipboard(String),
    RequestDraw(DrawPriority),
    Quit,
    Fatal(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirtyState {
    pub interaction: bool,
    pub content: bool,
    pub animation: bool,
}

impl DirtyState {
    pub const CLEAN: Self = Self {
        interaction: false,
        content: false,
        animation: false,
    };

    pub const fn interaction() -> Self {
        Self {
            interaction: true,
            ..Self::CLEAN
        }
    }

    pub const fn content() -> Self {
        Self {
            content: true,
            ..Self::CLEAN
        }
    }

    pub const fn animation() -> Self {
        Self {
            animation: true,
            ..Self::CLEAN
        }
    }

    pub fn merge(&mut self, other: Self) {
        self.interaction |= other.interaction;
        self.content |= other.content;
        self.animation |= other.animation;
    }

    pub const fn is_clean(self) -> bool {
        !self.interaction && !self.content && !self.animation
    }
}

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub actions: Vec<UiAction>,
    pub dirty: DirtyState,
    pub next_deadline: Option<Instant>,
}

impl Default for UpdateResult {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            dirty: DirtyState::CLEAN,
            next_deadline: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_state_merges_independent_frame_classes() {
        let mut dirty = DirtyState::interaction();
        dirty.merge(DirtyState::animation());
        assert!(dirty.interaction);
        assert!(!dirty.content);
        assert!(dirty.animation);
        assert!(!dirty.is_clean());
    }

    #[test]
    fn persist_config_action_owns_a_complete_snapshot() {
        let mut config = Config::default();
        config.theme = "ferra".into();
        let action = UiAction::PersistConfig(config.clone());
        config.theme = "deepseek-e".into();
        assert!(matches!(
            action,
            UiAction::PersistConfig(snapshot) if snapshot.theme == "ferra"
        ));
    }
}
