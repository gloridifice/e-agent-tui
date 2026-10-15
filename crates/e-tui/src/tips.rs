//! Provider-neutral tip definitions, registration and visible-time rotation.

use std::{
    collections::HashSet,
    fmt,
    time::{Duration, Instant},
};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    agent::CommandDescriptor,
    i18n::Language,
    key_mapping::{Action, KeyMapping, Scope},
};

mod shared;

pub const ROTATION_INTERVAL: Duration = Duration::from_secs(6);
pub const MIN_TIP_COLUMNS: usize = 24;

#[derive(Debug, Clone)]
pub struct TipDefinition {
    pub id: String,
    pub translations: Vec<TipTranslation>,
    pub requirements: Vec<TipRequirement>,
}

#[derive(Debug, Clone)]
pub struct TipTranslation {
    pub language: Language,
    pub segments: Vec<TipSegment>,
}

#[derive(Debug, Clone)]
pub enum TipSegment {
    Text(String),
    Keyword(String),
    KeyBinding { scope: Scope, action: Action },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TipPrerequisite {
    CompletedAnswer,
    AsapAdmission,
    PendingQueue,
    ShellAvailable,
    AttachedSession,
    Skills,
    Models,
    CurrentEfforts,
    AnyEfforts,
    ModelLetters,
    ModelMark,
    QuickLinks,
    Compaction,
    NewModes,
    UserMessages,
    NativeMcp,
    ErrorRecovery,
}

#[derive(Debug, Clone)]
pub enum TipRequirement {
    Command(String),
    Capability(String),
    Prerequisite(TipPrerequisite),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TipRegistrationError {
    InvalidId(String),
    DuplicateId(String),
    DuplicateLocale {
        id: String,
        language: Language,
    },
    InvalidBody {
        id: String,
        language: Option<Language>,
    },
    InvalidRequirement(String),
}

impl fmt::Display for TipRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Tips registration: {self:?}")
    }
}
impl std::error::Error for TipRegistrationError {}

impl TipDefinition {
    /// Backticks identify keywords; explicitly mapped keywords resolve live bindings.
    pub fn bilingual(
        id: &str,
        chinese: &str,
        english: &str,
        bindings: &[(&str, Scope, Action)],
        requirements: Vec<TipRequirement>,
    ) -> Self {
        let translation = |language, body: &str| TipTranslation {
            language,
            segments: body
                .split('`')
                .enumerate()
                .map(|(index, text)| {
                    if index % 2 == 0 {
                        TipSegment::Text(text.into())
                    } else if let Some((_, scope, action)) =
                        bindings.iter().find(|(label, _, _)| *label == text)
                    {
                        TipSegment::KeyBinding {
                            scope: *scope,
                            action: *action,
                        }
                    } else {
                        TipSegment::Keyword(text.into())
                    }
                })
                .collect(),
        };
        Self {
            id: id.into(),
            translations: vec![
                translation(Language::SimplifiedChinese, chinese),
                translation(Language::English, english),
            ],
            requirements,
        }
    }
}

pub(crate) struct TipContext<'a> {
    pub language: Language,
    pub bindings: &'a KeyMapping,
    pub commands: &'a [CommandDescriptor],
    pub prerequisites: &'a [TipPrerequisite],
}

#[derive(Debug)]
pub struct TipRegistry {
    definitions: Vec<TipDefinition>,
    capabilities: HashSet<String>,
}

fn printable(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}

impl Default for TipRegistry {
    fn default() -> Self {
        let mut registry = Self {
            definitions: Vec::new(),
            capabilities: HashSet::new(),
        };
        registry
            .register_tips(shared::definitions())
            .expect("valid shared Tips definitions");
        registry
    }
}

impl TipRegistry {
    pub fn register_tips(
        &mut self,
        tips: impl IntoIterator<Item = TipDefinition>,
    ) -> Result<(), TipRegistrationError> {
        let batch: Vec<_> = tips.into_iter().collect();
        let mut ids: HashSet<_> = self.definitions.iter().map(|tip| tip.id.as_str()).collect();
        for tip in &batch {
            if tip.id.trim().is_empty() || !printable(&tip.id) {
                return Err(TipRegistrationError::InvalidId(tip.id.clone()));
            }
            if !ids.insert(&tip.id) {
                return Err(TipRegistrationError::DuplicateId(tip.id.clone()));
            }
            if tip.translations.is_empty() {
                return Err(TipRegistrationError::InvalidBody {
                    id: tip.id.clone(),
                    language: None,
                });
            }
            let mut locales = HashSet::new();
            for translation in &tip.translations {
                if !locales.insert(translation.language) {
                    return Err(TipRegistrationError::DuplicateLocale {
                        id: tip.id.clone(),
                        language: translation.language,
                    });
                }
                let mut nonempty = false;
                for segment in &translation.segments {
                    match segment {
                        TipSegment::Text(text) | TipSegment::Keyword(text) => {
                            if !printable(text) {
                                return Err(TipRegistrationError::InvalidBody {
                                    id: tip.id.clone(),
                                    language: Some(translation.language),
                                });
                            }
                            nonempty |= !text.trim().is_empty();
                        }
                        TipSegment::KeyBinding { .. } => nonempty = true,
                    }
                }
                if !nonempty {
                    return Err(TipRegistrationError::InvalidBody {
                        id: tip.id.clone(),
                        language: Some(translation.language),
                    });
                }
            }
            for requirement in &tip.requirements {
                if let TipRequirement::Command(key) | TipRequirement::Capability(key) = requirement
                {
                    if key.trim().is_empty() || !printable(key) {
                        return Err(TipRegistrationError::InvalidRequirement(tip.id.clone()));
                    }
                }
            }
        }
        self.definitions.extend(batch);
        Ok(())
    }

    pub fn set_capability(&mut self, key: &str, available: bool) {
        if available {
            self.capabilities.insert(key.into());
        } else {
            self.capabilities.remove(key);
        }
    }

    pub fn clear_capabilities(&mut self) {
        self.capabilities.clear();
    }

    pub(crate) fn candidates(
        &self,
        context: TipContext<'_>,
        budget: usize,
    ) -> Vec<(usize, TipLine)> {
        if budget < MIN_TIP_COLUMNS {
            return Vec::new();
        }
        self.definitions
            .iter()
            .enumerate()
            .filter_map(|(index, tip)| {
                let eligible = tip
                    .requirements
                    .iter()
                    .all(|requirement| match requirement {
                        TipRequirement::Command(name) => {
                            crate::command_catalog::builtin_command(name).is_some()
                                || context.commands.iter().any(|command| command.name == *name)
                        }
                        TipRequirement::Capability(key) => self.capabilities.contains(key),
                        TipRequirement::Prerequisite(value) => {
                            context.prerequisites.contains(value)
                        }
                    });
                if !eligible {
                    return None;
                }
                let translation = tip
                    .translations
                    .iter()
                    .find(|translation| translation.language == context.language)?;
                let mut segments = vec![("Tips".to_owned(), true), (" ".to_owned(), false)];
                for segment in &translation.segments {
                    segments.push(match segment {
                        TipSegment::Text(text) => (text.clone(), false),
                        TipSegment::Keyword(text) => (text.clone(), true),
                        TipSegment::KeyBinding { scope, action } => {
                            let label = context.bindings.label(*scope, *action);
                            if label == "—" {
                                return None;
                            }
                            (label, true)
                        }
                    });
                }
                Some((index, TipLine::clip(segments, budget)))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TipLine {
    pub segments: Vec<(String, bool)>,
    pub width: usize,
    pub budget: usize,
}

impl TipLine {
    fn clip(segments: Vec<(String, bool)>, budget: usize) -> Self {
        let text: String = segments.iter().map(|(text, _)| text.as_str()).collect();
        let clipped = text.width() > budget;
        let limit = budget.saturating_sub(usize::from(clipped));
        let mut roles = Vec::new();
        let mut offset = 0;
        for (text, keyword) in segments {
            roles.push((offset, keyword));
            offset += text.len();
        }
        let mut result = Self {
            segments: Vec::new(),
            width: 0,
            budget,
        };
        for (byte, grapheme) in text.grapheme_indices(true) {
            let width = grapheme.width();
            if result.width + width > limit {
                break;
            }
            let keyword = roles
                .iter()
                .rev()
                .find(|(start, _)| *start <= byte)
                .is_some_and(|(_, keyword)| *keyword);
            if let Some((text, _)) = result
                .segments
                .last_mut()
                .filter(|(_, role)| *role == keyword)
            {
                text.push_str(grapheme);
            } else {
                result.segments.push((grapheme.into(), keyword));
            }
            result.width += width;
        }
        if clipped {
            result.segments.push(("…".into(), false));
            result.width += 1;
        }
        result
    }
}

#[derive(Debug)]
pub(crate) struct TipRotation {
    selected: Option<usize>,
    working: bool,
    remaining: Duration,
    pub deadline: Option<Instant>,
    advance: bool,
    pub line: Option<TipLine>,
}

impl Default for TipRotation {
    fn default() -> Self {
        Self {
            selected: None,
            working: false,
            remaining: ROTATION_INTERVAL,
            deadline: None,
            advance: false,
            line: None,
        }
    }
}

impl TipRotation {
    pub fn reconcile(
        &mut self,
        working: bool,
        candidates: Vec<(usize, TipLine)>,
        now: Instant,
    ) -> bool {
        let old = self.line.clone();
        if !working {
            *self = Self::default();
            return old.is_some();
        }
        if !self.working {
            *self = Self {
                working: true,
                ..Self::default()
            };
        }
        if candidates.is_empty() {
            if let Some(deadline) = self.deadline.take() {
                self.remaining = deadline.saturating_duration_since(now);
            }
            self.line = None;
            return old.is_some();
        }
        let current = candidates
            .iter()
            .position(|(index, _)| Some(*index) == self.selected);
        let next = if self.advance {
            current.map_or_else(
                || {
                    candidates
                        .iter()
                        .position(|(index, _)| Some(*index) > self.selected)
                        .unwrap_or(0)
                },
                |index| (index + 1) % candidates.len(),
            )
        } else {
            current.unwrap_or(0)
        };
        if self.advance || current.is_none() {
            self.remaining = ROTATION_INTERVAL;
            self.deadline = None;
        }
        self.advance = false;
        self.selected = Some(candidates[next].0);
        self.line = Some(candidates[next].1.clone());
        self.deadline.get_or_insert(now + self.remaining);
        old != self.line
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            self.remaining = ROTATION_INTERVAL;
            self.advance = true;
            return true;
        }
        false
    }
}
