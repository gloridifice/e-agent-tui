//! Pure keyboard schema, scoped resolution and presentation. No terminal or file I/O.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, LazyLock},
};

pub const DEFAULT_KEY_MAPPING_SOURCE: &str = include_str!("../assets/default_key_mapping.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    PrintHelp,
    EnterReadMode,
    ChooseModel,
    ChooseEffort,
    OpenSettings,
    ResumeSession,
    TogglePreview,
    CopyLink,
    PageUp,
    PageDown,
    NewLine,
    Paste,
    ClearOrQuit,
    CancelOrInterrupt,
    HistorySearch,
    Complete,
    ToggleMultiline,
    Send,
    SendAsap,
    SendAfterTurn,
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveUpFast,
    MoveDownFast,
    MoveUpHalf,
    MoveDownHalf,
    MoveStart,
    MoveEnd,
    DeleteBackward,
    DeleteForward,
    DeleteWordBackward,
    Previous,
    Next,
    Accept,
    Cancel,
    Exit,
    CopyBlock,
    EnterItems,
    BackToBlocks,
    Confirm,
    Back,
    Close,
    PreviousQuestion,
    NextQuestion,
    PreviousOption,
    NextOption,
    ToggleOption,
    Allow,
    Deny,
}

impl Action {
    pub fn name(self) -> String {
        toml::Value::try_from(self)
            .expect("action serializes")
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Global,
    Message,
    MessageIdle,
    MessageWorking,
    MessageEdit,
    MessageSuggest,
    MessageSearch,
    ReadMode,
    ReadModeItem,
    FullScreen,
    History,
    Page,
    PageEdit,
    PageChoice,
    PageResume,
    PageQuestion,
    PageQuestionEdit,
    Approval,
    Help,
}

impl Scope {
    pub const ALL: [Self; 19] = [
        Self::Global,
        Self::Message,
        Self::MessageIdle,
        Self::MessageWorking,
        Self::MessageEdit,
        Self::MessageSuggest,
        Self::MessageSearch,
        Self::ReadMode,
        Self::ReadModeItem,
        Self::FullScreen,
        Self::History,
        Self::Page,
        Self::PageEdit,
        Self::PageChoice,
        Self::PageResume,
        Self::PageQuestion,
        Self::PageQuestionEdit,
        Self::Approval,
        Self::Help,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Message => "message",
            Self::MessageIdle => "message.idle",
            Self::MessageWorking => "message.working",
            Self::MessageEdit => "message.edit",
            Self::MessageSuggest => "message.suggest",
            Self::MessageSearch => "message.search",
            Self::ReadMode => "read_mode",
            Self::ReadModeItem => "read_mode.item",
            Self::FullScreen => "full_screen",
            Self::History => "history",
            Self::Page => "page",
            Self::PageEdit => "page.edit",
            Self::PageChoice => "page.choice",
            Self::PageResume => "page.resume",
            Self::PageQuestion => "page.question",
            Self::PageQuestionEdit => "page.question.edit",
            Self::Approval => "approval",
            Self::Help => "help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Other,
}
impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Chord {
    pub fn parse(value: &str, platform: Platform) -> Result<Self, String> {
        let mut parts: Vec<_> = value.split('-').collect();
        let name = parts.pop().ok_or("empty key")?;
        let mut modifiers = KeyModifiers::NONE;
        for modifier in parts {
            let flag = match modifier.to_ascii_lowercase().as_str() {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                "super" => KeyModifiers::SUPER,
                "osmain" => {
                    if platform == Platform::Mac {
                        KeyModifiers::SUPER
                    } else {
                        KeyModifiers::CONTROL
                    }
                }
                _ => return Err(format!("unknown modifier {modifier:?}")),
            };
            if modifiers.contains(flag) {
                return Err(format!("duplicate modifier {modifier:?}"));
            }
            modifiers.insert(flag);
        }
        let lower = name.to_ascii_lowercase();
        let code = match lower.as_str() {
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "space" => KeyCode::Char(' '),
            "minus" => KeyCode::Char('-'),
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            _ if lower.starts_with('f')
                && lower[1..]
                    .parse::<u8>()
                    .is_ok_and(|n| (1..=12).contains(&n)) =>
            {
                KeyCode::F(lower[1..].parse().unwrap())
            }
            _ if name.chars().count() == 1 && !name.chars().next().unwrap().is_control() => {
                KeyCode::Char(name.chars().next().unwrap())
            }
            _ => return Err(format!("unknown key {name:?}")),
        };
        Ok(Self::normalize(code, modifiers))
    }

    fn normalize(mut code: KeyCode, mut modifiers: KeyModifiers) -> Self {
        if let KeyCode::Char(c) = code {
            if c.is_ascii_uppercase() {
                code = KeyCode::Char(c.to_ascii_lowercase());
                modifiers.insert(KeyModifiers::SHIFT);
            }
        }
        if code == KeyCode::BackTab {
            code = KeyCode::Tab;
            modifiers.insert(KeyModifiers::SHIFT);
        }
        Self { code, modifiers }
    }

    fn matches(self, key: &KeyEvent) -> bool {
        key.kind != KeyEventKind::Release && self == Self::normalize(key.code, key.modifiers)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, name) in [
            (KeyModifiers::CONTROL, "Ctrl+"),
            (KeyModifiers::ALT, "Alt+"),
            (KeyModifiers::SHIFT, "Shift+"),
            (KeyModifiers::SUPER, "Super+"),
        ] {
            if self.modifiers.contains(modifier) {
                f.write_str(name)?;
            }
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => write!(
                f,
                "{}",
                if self.modifiers.is_empty() {
                    c
                } else {
                    c.to_ascii_uppercase()
                }
            ),
            KeyCode::F(n) => write!(f, "F{n}"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            _ => write!(f, "{:?}", self.code),
        }
    }
}

pub fn text_character(key: &KeyEvent) -> Option<char> {
    if key.kind == KeyEventKind::Release
        || key.modifiers.intersects(
            KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::HYPER
                | KeyModifiers::META,
        )
    {
        return None;
    }
    match key.code {
        KeyCode::Char(c) if !c.is_control() => Some(c),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedKey {
    Command(Action),
    Text(char),
    MarkModel(char),
    SelectModel(char),
    Unbound,
}

type Bindings = BTreeMap<Action, Vec<Chord>>;
type Scopes = BTreeMap<Scope, Bindings>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMapping {
    source: Arc<Scopes>,
    effective: Arc<Scopes>,
    platform: Platform,
}

impl Default for KeyMapping {
    fn default() -> Self {
        static DEFAULT: LazyLock<KeyMapping> = LazyLock::new(|| {
            KeyMapping::from_user_toml_for("", Platform::current())
                .expect("valid embedded key mapping")
        });
        DEFAULT.clone()
    }
}

impl KeyMapping {
    pub fn from_user_toml(text: &str) -> Result<Self, String> {
        Self::from_user_toml_for(text, Platform::current())
    }

    pub fn from_user_toml_for(text: &str, platform: Platform) -> Result<Self, String> {
        let mut source: Scopes = Scope::ALL
            .into_iter()
            .map(|scope| (scope, BTreeMap::new()))
            .collect();
        Self::parse_document(DEFAULT_KEY_MAPPING_SOURCE, platform, None, &mut source)?;
        let schema = source.clone();
        Self::parse_document(text, platform, Some(&schema), &mut source)?;
        let mut effective = source.clone();
        for scope in [Scope::MessageIdle, Scope::MessageWorking] {
            let mut bindings = source[&Scope::Message].clone();
            bindings.extend(source[&Scope::MessageEdit].clone());
            bindings.extend(source[&scope].clone());
            effective.insert(scope, bindings);
        }
        let mut suggest = source[&Scope::Message].clone();
        suggest.remove(&Action::CancelOrInterrupt);
        suggest.extend(
            source[&Scope::MessageEdit]
                .iter()
                .filter(|(a, _)| !matches!(a, Action::MoveUp | Action::MoveDown))
                .map(|(a, b)| (*a, b.clone())),
        );
        suggest.extend(source[&Scope::MessageSuggest].clone());
        effective.insert(Scope::MessageSuggest, suggest);
        let mut search = source[&Scope::MessageSearch].clone();
        for action in [Action::Paste, Action::ClearOrQuit] {
            search.insert(action, source[&Scope::Message][&action].clone());
        }
        effective.insert(Scope::MessageSearch, search);
        let mut items = source[&Scope::ReadMode].clone();
        items.remove(&Action::EnterItems);
        items.extend(source[&Scope::ReadModeItem].clone());
        effective.insert(Scope::ReadModeItem, items);
        let mut history = source[&Scope::FullScreen].clone();
        history.extend(source[&Scope::History].clone());
        effective.insert(Scope::History, history);
        let mut help = source[&Scope::FullScreen].clone();
        help.remove(&Action::Exit);
        help.extend(source[&Scope::Help].clone());
        effective.insert(Scope::Help, help);
        let result = Self {
            source: Arc::new(source),
            effective: Arc::new(effective),
            platform,
        };
        result.validate()?;
        Ok(result)
    }

    fn parse_document(
        text: &str,
        platform: Platform,
        schema: Option<&Scopes>,
        out: &mut Scopes,
    ) -> Result<(), String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| format!("{e}"))?;
        fn visit(
            table: &toml::Table,
            prefix: &str,
            platform: Platform,
            schema: Option<&Scopes>,
            out: &mut Scopes,
        ) -> Result<(), String> {
            for (name, value) in table {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}.{name}")
                };
                if path == "history.toggle_view" {
                    continue;
                }
                if let Some(table) = value.as_table() {
                    if !Scope::ALL.iter().any(|s| s.name() == path) {
                        return Err(format!("{path}: unknown scope"));
                    }
                    visit(table, &path, platform, schema, out)?;
                    continue;
                }
                let scope = Scope::ALL
                    .into_iter()
                    .find(|s| s.name() == prefix)
                    .ok_or_else(|| format!("{path}: expected a scope table"))?;
                let action: Action = toml::Value::String(name.clone())
                    .try_into()
                    .map_err(|_| format!("{path}: unknown action"))?;
                if schema.is_some_and(|s| !s.get(&scope).is_some_and(|b| b.contains_key(&action))) {
                    return Err(format!("{path}: action is not available in this scope"));
                }
                let values: Vec<&str> = match value {
                    toml::Value::String(s) => vec![s],
                    toml::Value::Array(a) => a
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .ok_or_else(|| format!("{path}: expected a string array"))
                        })
                        .collect::<Result<_, _>>()?,
                    _ => return Err(format!("{path}: expected a key string or string array")),
                };
                let mut chords = Vec::new();
                for value in &values {
                    if *value == "nop" {
                        if values.len() != 1 {
                            return Err(format!("{path}: nop cannot be combined with keys"));
                        }
                    } else {
                        let chord =
                            Chord::parse(value, platform).map_err(|e| format!("{path}: {e}"))?;
                        if !chords.contains(&chord) {
                            chords.push(chord);
                        }
                    }
                }
                out.entry(scope).or_default().insert(action, chords);
            }
            Ok(())
        }
        visit(&table, "", platform, schema, out)
    }

    fn binding_path(&self, scope: Scope, action: Action) -> String {
        let inherited = if matches!(scope, Scope::History | Scope::Help)
            && self.source[&Scope::FullScreen].contains_key(&action)
        {
            Some(Scope::FullScreen)
        } else {
            [Scope::Message, Scope::MessageEdit, Scope::ReadMode]
                .into_iter()
                .find(|candidate| self.source[candidate].contains_key(&action))
        };
        let owner = self.source[&scope]
            .contains_key(&action)
            .then_some(scope)
            .or(inherited)
            .expect("effective bindings have a source owner");
        format!("{}.{}", owner.name(), action.name())
    }

    fn validate(&self) -> Result<(), String> {
        for (&scope, bindings) in self.effective.iter() {
            let mut seen: Vec<(Chord, String)> = Vec::new();
            for (&action, chords) in bindings {
                let path = self.binding_path(scope, action);
                for &chord in chords {
                    if let Some((_, other)) = seen.iter().find(|(c, _)| *c == chord) {
                        return Err(format!("{path} conflicts with {other}: {chord}"));
                    }
                    seen.push((chord, path.clone()));
                    if scope != Scope::Global {
                        for (&global, keys) in &self.source[&Scope::Global] {
                            if !Self::global_is_active(scope, global) {
                                continue;
                            }
                            if scope == Scope::Help
                                && action == Action::Close
                                && global == Action::PrintHelp
                            {
                                continue;
                            }
                            if keys.contains(&chord) {
                                return Err(format!(
                                    "{path} conflicts with global.{}: {chord}",
                                    global.name()
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn global_is_active(scope: Scope, action: Action) -> bool {
        !(matches!(
            scope,
            Scope::ReadMode
                | Scope::ReadModeItem
                | Scope::FullScreen
                | Scope::History
                | Scope::Help
        ) && matches!(action, Action::PageUp | Action::PageDown))
    }

    pub fn resolve_global(&self, scope: Scope, key: &KeyEvent) -> Option<Action> {
        self.resolve(Scope::Global, key)
            .filter(|&action| Self::global_is_active(scope, action))
    }

    pub fn resolve(&self, scope: Scope, key: &KeyEvent) -> Option<Action> {
        self.effective
            .get(&scope)?
            .iter()
            .find_map(|(&action, keys)| keys.iter().any(|c| c.matches(key)).then_some(action))
    }

    pub fn input(&self, scope: Scope, key: &KeyEvent) -> MappedKey {
        self.resolve(scope, key)
            .map(MappedKey::Command)
            .or_else(|| text_character(key).map(MappedKey::Text))
            .unwrap_or(MappedKey::Unbound)
    }

    pub fn model_letter_available(&self, letter: char) -> bool {
        letter.is_ascii_lowercase()
            && [KeyModifiers::NONE, KeyModifiers::SHIFT]
                .into_iter()
                .all(|modifiers| {
                    let key = KeyEvent::new(KeyCode::Char(letter), modifiers);
                    self.resolve(Scope::Page, &key).is_none()
                        && self.resolve_global(Scope::Page, &key).is_none()
                })
    }

    pub fn model_input(&self, key: &KeyEvent) -> MappedKey {
        let chord = Chord::normalize(key.code, key.modifiers);
        if key.kind != KeyEventKind::Release {
            if let KeyCode::Char(letter) = chord.code {
                if self.model_letter_available(letter) {
                    if chord.modifiers == KeyModifiers::SHIFT {
                        return MappedKey::MarkModel(letter);
                    }
                    if chord.modifiers.is_empty() {
                        return MappedKey::SelectModel(letter);
                    }
                }
            }
        }
        self.input(Scope::Page, key)
    }

    pub fn label(&self, scope: Scope, action: Action) -> String {
        self.effective
            .get(&scope)
            .and_then(|b| b.get(&action))
            .filter(|v| !v.is_empty())
            .map(|keys| {
                keys.iter()
                    .map(|k| {
                        let label = k.to_string();
                        if self.platform == Platform::Mac {
                            label.replace("Super+", "⌘").replace("Alt+", "⌥")
                        } else {
                            label
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_else(|| "—".into())
    }

    pub fn entries(&self) -> impl Iterator<Item = (Scope, Action)> + '_ {
        self.source
            .iter()
            .flat_map(|(&scope, actions)| actions.keys().map(move |&action| (scope, action)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }
    #[test]
    fn key_mapping_model_letters_respect_chords_and_effective_bindings() {
        let map = KeyMapping::default();
        for (character, modifiers) in [
            ('A', KeyModifiers::NONE),
            ('A', KeyModifiers::SHIFT),
            ('a', KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                map.model_input(&key(KeyCode::Char(character), modifiers)),
                MappedKey::MarkModel('a')
            );
        }
        assert_eq!(
            map.model_input(&key(KeyCode::Char('a'), KeyModifiers::NONE)),
            MappedKey::SelectModel('a')
        );
        for letter in "hjklq".chars() {
            assert!(!map.model_letter_available(letter));
            assert_eq!(
                map.model_input(&key(KeyCode::Char(letter), KeyModifiers::SHIFT)),
                MappedKey::Text(letter)
            );
        }
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SUPER,
            KeyModifiers::SHIFT | KeyModifiers::CONTROL,
        ] {
            assert_eq!(
                map.model_input(&key(KeyCode::Char('a'), modifiers)),
                MappedKey::Unbound
            );
        }
        for character in ['中', 'é', '1'] {
            assert!(!matches!(
                map.model_input(&key(KeyCode::Char(character), KeyModifiers::SHIFT)),
                MappedKey::MarkModel(_) | MappedKey::SelectModel(_)
            ));
        }
        let mut release = key(KeyCode::Char('A'), KeyModifiers::SHIFT);
        release.kind = KeyEventKind::Release;
        assert_eq!(map.model_input(&release), MappedKey::Unbound);
        let custom = KeyMapping::from_user_toml("[page]\nmove_down='nop'\nconfirm='a'\nback='shift-b'\n[global]\ntoggle_preview='c'\nchoose_model='shift-d'").unwrap();
        assert!(custom.model_letter_available('j'));
        for letter in "abcd".chars() {
            assert!(!custom.model_letter_available(letter));
        }
        assert_eq!(
            custom.model_input(&key(KeyCode::Char('a'), KeyModifiers::NONE)),
            MappedKey::Command(Action::Confirm)
        );
        assert_eq!(
            map.input(Scope::Page, &key(KeyCode::Char('a'), KeyModifiers::NONE)),
            MappedKey::Text('a')
        );
    }

    #[test]
    fn key_mapping_defaults_and_platforms() {
        for platform in [Platform::Mac, Platform::Other] {
            let map = KeyMapping::from_user_toml_for("", platform).unwrap();
            let modifier = if platform == Platform::Mac {
                KeyModifiers::SUPER
            } else {
                KeyModifiers::CONTROL
            };
            assert_eq!(
                map.resolve(Scope::Global, &key(KeyCode::Char('r'), modifier)),
                Some(Action::EnterReadMode)
            );
            assert_eq!(
                map.resolve(
                    Scope::Global,
                    &key(KeyCode::Char('r'), modifier | KeyModifiers::SHIFT)
                ),
                None
            );
            assert_eq!(
                map.resolve(Scope::ReadModeItem, &key(KeyCode::Esc, KeyModifiers::NONE)),
                Some(Action::Exit)
            );
        }
    }
    #[test]
    fn key_mapping_overlay_disable_and_conflicts() {
        let map = KeyMapping::from_user_toml_for(
            "[global]\nchoose_model='f1'\n[read_mode.item]\nmove_up='nop'",
            Platform::Other,
        )
        .unwrap();
        assert_eq!(
            map.resolve(
                Scope::Global,
                &key(KeyCode::Char('l'), KeyModifiers::CONTROL)
            ),
            None
        );
        assert_eq!(
            map.resolve(Scope::Global, &key(KeyCode::F(1), KeyModifiers::NONE)),
            Some(Action::ChooseModel)
        );
        assert_eq!(
            map.resolve(Scope::ReadModeItem, &key(KeyCode::Up, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            map.resolve(Scope::ReadMode, &key(KeyCode::Up, KeyModifiers::NONE)),
            Some(Action::MoveUp)
        );
        for invalid in [
            "[global]\nchoose_model='osmain-h'",
            "[read_mode]\ncopy_block='osmain-h'",
            "[bad]",
            "[message]\nsend='enter'",
            "[global]\nprint_help=['nop','f1']",
            "[message]\npaste=123",
        ] {
            assert!(KeyMapping::from_user_toml(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn key_mapping_conflicts_report_editable_source_paths() {
        let error = KeyMapping::from_user_toml("[message]\npaste='up'").unwrap_err();
        assert!(error.contains("message.paste"));
        assert!(error.contains("message.edit.move_up"));
        assert!(!error.contains("message.idle.move_up"));
    }

    #[test]
    fn history_scope_ignores_retired_toggle_and_keeps_navigation_overrides() {
        let mapping = KeyMapping::from_user_toml_for(
            "[full_screen]\nexit='nop'\nmove_down='x'\n[history]\ntoggle_view='x'",
            Platform::Other,
        )
        .unwrap();
        assert_eq!(
            mapping.resolve(Scope::History, &key(KeyCode::Char('x'), KeyModifiers::NONE)),
            Some(Action::MoveDown)
        );
        assert_eq!(
            mapping.resolve(Scope::History, &key(KeyCode::Tab, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            mapping.resolve(Scope::History, &key(KeyCode::Char('q'), KeyModifiers::NONE)),
            None
        );
        assert_eq!(mapping.label(Scope::History, Action::Exit), "—");
        assert!(!mapping
            .entries()
            .any(|(_, action)| action.name() == "toggle_view"));
    }

    #[test]
    fn key_mapping_normalizes_shift_and_rejects_release() {
        let map = KeyMapping::default();
        assert_eq!(
            map.resolve(
                Scope::Approval,
                &key(KeyCode::Char('Y'), KeyModifiers::NONE)
            ),
            Some(Action::Allow)
        );
        assert_eq!(
            map.resolve(
                Scope::Approval,
                &KeyEvent::new_with_kind(
                    KeyCode::Char('y'),
                    KeyModifiers::NONE,
                    KeyEventKind::Release
                )
            ),
            None
        );
        assert!(Chord::parse("ctrl-ctrl-a", Platform::Other).is_err());
        assert!(Chord::parse("g g", Platform::Other).is_err());
        assert!(Chord::parse("ctrl-minus", Platform::Other).is_ok());
    }
}
