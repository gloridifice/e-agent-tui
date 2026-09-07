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

#[derive(Debug, Clone, PartialEq, Eq)]
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
        config.theme = "deepseek-e".into();
        assert!(matches!(
            action,
            UiAction::PersistConfig(snapshot) if snapshot.theme == "ferra"
        ));
    }
}
