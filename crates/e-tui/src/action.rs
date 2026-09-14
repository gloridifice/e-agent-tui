//! Owned requests returned by synchronous frontend updates.

use std::time::Instant;

use unicode_segmentation::UnicodeSegmentation;

use crate::{
    i18n::{tr_args, Language},
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
pub struct PromptImage {
    pub media_type: String,
    pub data: Vec<u8>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPart {
    Text(String),
    Image(PromptImage),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptInput {
    pub parts: Vec<PromptPart>,
}

impl From<String> for PromptInput {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl From<&str> for PromptInput {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl PartialEq<&str> for PromptInput {
    fn eq(&self, other: &&str) -> bool {
        self.plain_text() == Some(*other)
    }
}

impl PromptInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![PromptPart::Text(text.into())],
        }
    }

    pub fn plain_text(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [PromptPart::Text(text)] => Some(text),
            _ => None,
        }
    }

    pub fn skill_name(&self) -> Option<&str> {
        let text = self.plain_text()?.trim();
        let name = text
            .strip_prefix("/skill:")
            .or_else(|| text.strip_prefix("/skill "))?;
        name.split_whitespace().next()
    }

    pub fn display_text(&self) -> String {
        self.display_text_in(Language::English)
    }

    pub fn display_text_in(&self, language: Language) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                PromptPart::Text(text) => text.clone(),
                PromptPart::Image(image) => tr_args(
                    language,
                    "composer.image",
                    &[(
                        "name",
                        image.name.as_deref().unwrap_or("clipboard.png").to_owned(),
                    )],
                ),
            })
            .collect()
    }

    pub fn has_images(&self) -> bool {
        self.parts
            .iter()
            .any(|part| matches!(part, PromptPart::Image(_)))
    }

    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|part| match part {
            PromptPart::Text(text) => text.is_empty(),
            PromptPart::Image(_) => false,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardPaste {
    Text(String),
    Image(PromptImage),
}

#[derive(Clone, PartialEq, Eq)]
pub enum AgentRequest {
    Input {
        prompt: PromptInput,
    },
    Steer {
        prompt: PromptInput,
    },
    ClearAsap,
    NewInput {
        mode: String,
        prompt: PromptInput,
    },
    Command {
        line: String,
        images: Vec<PromptImage>,
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
    AuthGet {
        provider_ref: Option<String>,
        logout: bool,
    },
    AuthStart {
        provider: String,
        method: Option<String>,
        logout: bool,
    },
    AuthReply {
        flow_id: String,
        prompt_id: String,
        value: String,
    },
    AuthOpenUrl {
        flow_id: String,
        url: String,
    },
    AuthCancel,
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
        reasoning_effort: Option<String>,
    },
    Ping,
}

impl std::fmt::Debug for AgentRequest {
    /// Names the variant so requests stay diagnosable, and never formats a
    /// credential or provider answer.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const REDACTED: &str = "<redacted>";
        match self {
            Self::Input { prompt } => formatter.debug_tuple("Input").field(prompt).finish(),
            Self::Steer { prompt } => formatter.debug_tuple("Steer").field(prompt).finish(),
            Self::ClearAsap => formatter.write_str("ClearAsap"),
            Self::NewInput { mode, prompt } => formatter
                .debug_struct("NewInput")
                .field("mode", mode)
                .field("prompt", prompt)
                .finish(),
            Self::Command { line, images } => formatter
                .debug_struct("Command")
                .field("line", line)
                .field("images", &images.len())
                .finish(),
            Self::Interrupt => formatter.write_str("Interrupt"),
            Self::Attach { session_id } => formatter
                .debug_struct("Attach")
                .field("session_id", session_id)
                .finish(),
            Self::ListSessions => formatter.write_str("ListSessions"),
            Self::ApprovalAnswer { id, allow } => formatter
                .debug_struct("ApprovalAnswer")
                .field("id", id)
                .field("allow", allow)
                .finish(),
            Self::AnswerQuestions {
                request_id,
                answers,
            } => formatter
                .debug_struct("AnswerQuestions")
                .field("request_id", request_id)
                .field("answers", &answers.len())
                .finish(),
            Self::CancelQuestions { request_id } => formatter
                .debug_struct("CancelQuestions")
                .field("request_id", request_id)
                .finish(),
            Self::History {
                before_sequence,
                limit,
            } => formatter
                .debug_struct("History")
                .field("before_sequence", before_sequence)
                .field("limit", limit)
                .finish(),
            Self::LoginGet => formatter.write_str("LoginGet"),
            Self::AuthGet {
                provider_ref,
                logout,
            } => formatter
                .debug_struct("AuthGet")
                .field("provider_ref", provider_ref)
                .field("logout", logout)
                .finish(),
            Self::AuthStart {
                provider,
                method,
                logout,
            } => formatter
                .debug_struct("AuthStart")
                .field("provider", provider)
                .field("method", method)
                .field("logout", logout)
                .finish(),
            Self::AuthReply {
                flow_id,
                prompt_id,
                value: _,
            } => formatter
                .debug_struct("AuthReply")
                .field("flow_id", flow_id)
                .field("prompt_id", prompt_id)
                .field("value", &REDACTED)
                .finish(),
            Self::AuthOpenUrl { flow_id, url } => formatter
                .debug_struct("AuthOpenUrl")
                .field("flow_id", flow_id)
                .field("url", url)
                .finish(),
            Self::AuthCancel => formatter.write_str("AuthCancel"),
            Self::LoginSetApiKey { provider, value: _ } => formatter
                .debug_struct("LoginSetApiKey")
                .field("provider", provider)
                .field("value", &REDACTED)
                .finish(),
            Self::LoginProxyCreate {
                base_url,
                api_key: _,
                protocol,
                model,
            } => formatter
                .debug_struct("LoginProxyCreate")
                .field("base_url", base_url)
                .field("api_key", &REDACTED)
                .field("protocol", protocol)
                .field("model", model)
                .finish(),
            Self::LoginProxyDelete { id } => formatter
                .debug_struct("LoginProxyDelete")
                .field("id", id)
                .finish(),
            Self::ModelGet => formatter.write_str("ModelGet"),
            Self::ModelSet {
                provider,
                model,
                reasoning_effort,
            } => formatter
                .debug_struct("ModelSet")
                .field("provider", provider)
                .field("model", model)
                .field("reasoning_effort", reasoning_effort)
                .finish(),
            Self::Ping => formatter.write_str("Ping"),
        }
    }
}

/// Produce a single-line, grapheme-safe preview for clipboard feedback.
pub fn clipboard_preview(text: &str, limit: usize) -> (String, bool) {
    let mut graphemes = text.graphemes(true);
    let preview = graphemes
        .by_ref()
        .take(limit)
        .map(|grapheme| {
            if grapheme.chars().any(char::is_whitespace) {
                " "
            } else {
                grapheme
            }
        })
        .collect::<String>();
    (preview, graphemes.next().is_some())
}

#[derive(Debug, Clone)]
pub enum EffectResult {
    LinksValidated {
        request: crate::link_copy::LinkValidationRequest,
        validations: Vec<crate::link_copy::CandidateGroupValidation>,
    },
    PathsCompleted {
        request: crate::path_completion::PathCompletionRequest,
        candidates: Vec<crate::path_completion::PathCandidate>,
    },
    ConfigPersisted(Result<(), String>),
    ConfigReloaded {
        config: Box<Config>,
        themes: Vec<ThemeFile>,
    },
    ConfigReloadFailed(String),
    ClipboardRead(Result<ClipboardPaste, String>),
    ClipboardWritten {
        lines: usize,
        preview: String,
        truncated: bool,
    },
    ClipboardFailed(String),
    HistoryQueried {
        request: crate::execution_history::HistoryQueryRequest,
        result: Result<crate::execution_history::HistoryQueryResult, String>,
    },
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
    ValidateLinks(crate::link_copy::LinkValidationRequest),
    CompletePaths(crate::path_completion::PathCompletionRequest),
    Agent(AgentRequest),
    ResolvePreview(PreviewRequest),
    QueryHistory(crate::execution_history::HistoryQueryRequest),
    PersistConfig(Config),
    ReloadConfig,
    PersistSessionId(String),
    ReadClipboard,
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
    fn clipboard_preview_is_grapheme_safe_single_line_and_reports_truncation() {
        assert_eq!(
            clipboard_preview("A界🙂éZ\nmore", 6),
            ("A界🙂éZ ".into(), true)
        );
        assert_eq!(clipboard_preview("short", 6), ("short".into(), false));
    }

    #[test]
    fn persist_config_action_owns_a_complete_snapshot() {
        let mut config = Config::default();
        config.theme = "ferra".into();
        let action = UiAction::PersistConfig(config.clone());
        config.theme = "dracula".into();
        assert!(matches!(
            action,
            UiAction::PersistConfig(snapshot) if snapshot.theme == "ferra"
        ));
    }
}
