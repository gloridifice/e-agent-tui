//! /settings Input Page: category tabs are display-only while editable rows
//! share one visible focus. The page replaces the ordinary input area without
//! a floating border.

#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    config::{Config, HexRgb, PaneWidthPercent, RevealRate},
    page_core::{handle_text_input, TextEditResult, TextEditor},
};

pub const CATEGORIES: &[&str] = &[
    "settings.category.appearance",
    "settings.category.behavior",
    "settings.category.display",
    "settings.category.advanced",
];

pub type ChoiceOption = (&'static str, &'static str);

/// Value kinds. Choice values are stable config values; labels are translation
/// keys resolved only by the settings renderer.
#[derive(Clone, Copy, PartialEq)]
pub enum ItemKind {
    Choice {
        options: &'static [ChoiceOption],
    },
    /// Choice over the live agent-preset roster (`/new` modes): the options
    /// are not static — they come from `SettingsState.modes`, fed by the
    /// bridge's `presets` message.
    ModeChoice,
    /// Choice over the discovered theme files (`%APPDATA%\dshe\themes\`):
    /// options come from `SettingsState.themes`.
    ThemeChoice,
    Input,
    ReadOnly,
}

pub struct ItemDef {
    pub category: usize,
    /// Stable setting identity used by focus and edit reconciliation.
    pub key: &'static str,
    /// Translation key for the rendered label.
    pub label: &'static str,
    /// Translation key for the rendered description.
    pub desc: &'static str,
    pub kind: ItemKind,
    /// Read the current value as a display string (an option label for
    /// choices, the number for inputs).
    pub get: fn(&Config) -> String,
    /// Apply a confirmed value (an option label or the typed number).
    pub apply: fn(&mut Config, value: String),
}

const BOOL_OPTIONS: &[ChoiceOption] = &[("on", "common.on"), ("off", "common.off")];
const ALIGN_OPTIONS: &[ChoiceOption] = &[
    ("center", "settings.choice.align.center"),
    ("left", "settings.choice.align.left"),
    ("right", "settings.choice.align.right"),
];
const THINKING_OPTIONS: &[ChoiceOption] = &[
    ("compact", "settings.choice.thinking.compact"),
    ("lines", "settings.choice.thinking.lines"),
    ("full", "settings.choice.thinking.full"),
];
const LANGUAGE_OPTIONS: &[ChoiceOption] = &[
    ("en", "settings.choice.language.en"),
    ("zh-CN", "settings.choice.language.zh_cn"),
];

pub static ITEMS: &[ItemDef] = &[
    ItemDef {
        category: 0,
        key: "theme",
        label: "settings.item.theme.label",
        desc: "settings.item.theme.desc",
        kind: ItemKind::ThemeChoice,
        get: |c| c.theme.clone(),
        apply: |c, v| {
            if !v.is_empty() {
                c.theme = v;
            }
        },
    },
    ItemDef {
        category: 0,
        key: "plain_color",
        label: "settings.item.plain_color.label",
        desc: "settings.item.plain_color.desc",
        kind: ItemKind::Choice {
            options: BOOL_OPTIONS,
        },
        get: |c| bool_value(c.plain_color),
        apply: |c, v| {
            c.plain_color = v == "on";
        },
    },
    ItemDef {
        category: 0,
        key: "background_color",
        label: "settings.item.background_color.label",
        desc: "settings.item.background_color.desc",
        kind: ItemKind::Input,
        get: |c| c.background_color.to_string(),
        apply: |c, v| {
            if let Ok(color) = v.parse::<HexRgb>() {
                c.background_color = color;
            }
        },
    },
    ItemDef {
        category: 0,
        key: "user_input_padding",
        label: "settings.item.user_input_padding.label",
        desc: "settings.item.user_input_padding.desc",
        kind: ItemKind::Input,
        get: |c| c.user_input_padding.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse::<usize>() {
                c.user_input_padding = n.min(16);
            }
        },
    },
    ItemDef {
        category: 0,
        key: "message_pane_percent",
        label: "settings.item.message_pane_percent.label",
        desc: "settings.item.message_pane_percent.desc",
        kind: ItemKind::Input,
        get: |c| c.message_pane_percent.display(),
        apply: |c, v| {
            let value = v.trim().trim_end_matches('%').trim();
            if let Ok(percent) = value.parse::<f64>() {
                if let Ok(percent) = PaneWidthPercent::from_percent(percent) {
                    c.message_pane_percent = percent;
                }
            }
        },
    },
    ItemDef {
        category: 0,
        key: "page_max_width",
        label: "settings.item.page_max_width.label",
        desc: "settings.item.page_max_width.desc",
        kind: ItemKind::Input,
        get: |c| c.page_max_width.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse::<usize>() {
                c.page_max_width = n.min(500);
            }
        },
    },
    ItemDef {
        category: 0,
        key: "page_align",
        label: "settings.item.page_align.label",
        desc: "settings.item.page_align.desc",
        kind: ItemKind::Choice {
            options: ALIGN_OPTIONS,
        },
        get: |c| c.page_align_value().into(),
        apply: |c, v| {
            c.page_align = match v.as_str() {
                "left" | "right" | "center" => v,
                _ => "center".into(),
            };
        },
    },
    ItemDef {
        category: 1,
        key: "remember_last_session",
        label: "settings.item.remember_last_session.label",
        desc: "settings.item.remember_last_session.desc",
        kind: ItemKind::Choice {
            options: BOOL_OPTIONS,
        },
        get: |c| bool_value(c.remember_last_session),
        apply: |c, v| {
            c.remember_last_session = v == "on";
        },
    },
    ItemDef {
        category: 1,
        key: "language",
        label: "settings.item.language.label",
        desc: "settings.item.language.desc",
        kind: ItemKind::Choice {
            options: LANGUAGE_OPTIONS,
        },
        get: |c| c.language.to_string(),
        apply: |c, v| {
            if let Ok(language) = v.parse() {
                c.language = language;
            }
        },
    },
    ItemDef {
        category: 1,
        key: "default_mode",
        label: "settings.item.default_mode.label",
        desc: "settings.item.default_mode.desc",
        kind: ItemKind::ModeChoice,
        get: |c| c.default_mode.clone(),
        apply: |c, v| {
            if !v.is_empty() {
                c.default_mode = v;
            }
        },
    },
    ItemDef {
        category: 1,
        key: "paste_placeholder_chars",
        label: "settings.item.paste_placeholder_chars.label",
        desc: "settings.item.paste_placeholder_chars.desc",
        kind: ItemKind::Input,
        get: |c| c.paste_placeholder_chars.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse() {
                c.paste_placeholder_chars = n;
            }
        },
    },
    ItemDef {
        category: 1,
        key: "copy_toast_secs",
        label: "settings.item.copy_toast_secs.label",
        desc: "settings.item.copy_toast_secs.desc",
        kind: ItemKind::Input,
        get: |c| c.copy_toast_secs.max(3).to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse::<u64>() {
                c.copy_toast_secs = n.max(3);
            }
        },
    },
    ItemDef {
        category: 1,
        key: "history_limit",
        label: "settings.item.history_limit.label",
        desc: "settings.item.history_limit.desc",
        kind: ItemKind::Input,
        get: |c| c.history_limit.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse() {
                c.history_limit = n;
            }
        },
    },
    ItemDef {
        category: 2,
        key: "show_tool_duration",
        label: "settings.item.show_tool_duration.label",
        desc: "settings.item.show_tool_duration.desc",
        kind: ItemKind::Choice {
            options: BOOL_OPTIONS,
        },
        get: |c| bool_value(c.show_tool_duration),
        apply: |c, v| {
            c.show_tool_duration = v == "on";
        },
    },
    ItemDef {
        category: 2,
        key: "read_merge",
        label: "settings.item.read_merge.label",
        desc: "settings.item.read_merge.desc",
        kind: ItemKind::Choice {
            options: BOOL_OPTIONS,
        },
        get: |c| bool_value(c.read_merge),
        apply: |c, v| {
            c.read_merge = v == "on";
        },
    },
    ItemDef {
        category: 2,
        key: "thinking_display",
        label: "settings.item.thinking_display.label",
        desc: "settings.item.thinking_display.desc",
        kind: ItemKind::Choice {
            options: THINKING_OPTIONS,
        },
        get: |c| c.thinking_display_value().into(),
        apply: |c, v| {
            c.thinking_display = match v.as_str() {
                "lines" | "full" | "compact" => v,
                _ => "compact".into(),
            };
        },
    },
    ItemDef {
        category: 2,
        key: "thinking_lines",
        label: "settings.item.thinking_lines.label",
        desc: "settings.item.thinking_lines.desc",
        kind: ItemKind::Input,
        get: |c| c.thinking_lines.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse::<usize>() {
                c.thinking_lines = n.clamp(1, 50);
            }
        },
    },
    ItemDef {
        category: 2,
        key: "mermaid_enabled",
        label: "settings.item.mermaid_enabled.label",
        desc: "settings.item.mermaid_enabled.desc",
        kind: ItemKind::Choice {
            options: BOOL_OPTIONS,
        },
        get: |c| bool_value(c.mermaid_enabled),
        apply: |c, v| {
            c.mermaid_enabled = v == "on";
        },
    },
    ItemDef {
        category: 2,
        key: "message_chars_per_second",
        label: "settings.item.message_speed.label",
        desc: "settings.item.message_speed.desc",
        kind: ItemKind::Input,
        get: |c| c.message_chars_per_second.to_string(),
        apply: |c, v| {
            if let Ok(rate) = v.parse::<RevealRate>() {
                c.message_chars_per_second = rate;
            }
        },
    },
    ItemDef {
        category: 2,
        key: "preview_lines_per_second",
        label: "settings.item.preview_speed.label",
        desc: "settings.item.preview_speed.desc",
        kind: ItemKind::Input,
        get: |c| c.preview_lines_per_second.to_string(),
        apply: |c, v| {
            if let Ok(rate) = v.parse::<RevealRate>() {
                c.preview_lines_per_second = rate;
            }
        },
    },
    ItemDef {
        category: 3,
        key: "config_path_display",
        label: "settings.item.config_path.label",
        desc: "settings.item.config_path.desc",
        kind: ItemKind::ReadOnly,
        get: |config| config.config_path_display.clone(),
        apply: |_, _| {},
    },
];

fn bool_value(value: bool) -> String {
    if value {
        "on".into()
    } else {
        "off".into()
    }
}

/// Resolve a static choice value into its localized presentation label.
pub fn option_label(def: &ItemDef, value: &str, language: crate::Language) -> String {
    match def.kind {
        ItemKind::Choice { options } => options
            .iter()
            .find(|(candidate, _)| *candidate == value)
            .map(|(_, key)| crate::i18n::tr(language, key))
            .unwrap_or_else(|| value.to_owned()),
        ItemKind::ModeChoice | ItemKind::ThemeChoice | ItemKind::Input | ItemKind::ReadOnly => {
            value.to_owned()
        }
    }
}

/// Items of one category page, in declaration order.
pub fn items_in(category: usize) -> Vec<&'static ItemDef> {
    ITEMS.iter().filter(|i| i.category == category).collect()
}

/// Option list of a choice/mode/theme-choice item: the static labels, the
/// live mode roster (`SettingsState.modes`, the ids of the presets the
/// bridge sent), or the live theme list (`SettingsState.themes`). The
/// current value is appended when the roster lacks it, so a configured mode
/// or theme that no longer exists stays visible (the bridge falls back to
/// `standard`; the theme resolver falls back to a built-in).
pub fn dynamic_options(
    def: &ItemDef,
    config: &Config,
    modes: &[String],
    themes: &[String],
) -> Vec<String> {
    match def.kind {
        ItemKind::Choice { options } => options
            .iter()
            .map(|(value, _)| (*value).to_string())
            .collect(),
        ItemKind::ModeChoice => {
            let mut list: Vec<String> = modes.to_vec();
            if !list.iter().any(|m| *m == config.default_mode) {
                list.push(config.default_mode.clone());
            }
            list
        }
        ItemKind::ThemeChoice => {
            let mut list: Vec<String> = themes.to_vec();
            if !list.iter().any(|t| *t == config.theme) {
                list.push(config.theme.clone());
            }
            list
        }
        _ => Vec::new(),
    }
}

/// In-progress edit of the focused value. Enter confirms, Esc cancels;
/// nothing is applied until confirmation.
#[derive(Debug, PartialEq)]
pub enum Edit {
    /// Number input: the typed buffer (starts empty — replace semantics).
    Input { buf: String },
    /// Choice input: cursor index over the options.
    Choice { cursor: usize },
}

pub struct SettingsState {
    /// Current category page. Left/Right and h/l switch it directly; the
    /// category strip itself never enters the focus graph.
    pub category: usize,
    /// Hovered item index, remembered per category page.
    pub pos: [usize; 4],
    /// Active edit (only when the hovered item is being edited).
    pub editing: Option<Edit>,
    /// First visible row of the items area (display-only; the renderer
    /// re-anchors it so the focused item stays visible).
    pub scroll: usize,
    /// Agent-preset mode ids from the bridge's `presets` roster — the
    /// option list of the 默认模式 item.
    pub modes: Vec<String>,
    /// Theme names discovered from the themes directory — the option list
    /// of the 主题 item.
    pub themes: Vec<String>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            category: 0,
            pos: [0; 4],
            editing: None,
            scroll: 0,
            modes: Vec::new(),
            themes: Vec::new(),
        }
    }
}

pub enum SettingsAction {
    None,
    /// Config changed; caller persists and refreshes derived state.
    Changed,
    Exit,
}

impl SettingsState {
    pub fn current_item(&self) -> Option<&'static ItemDef> {
        items_in(self.category)
            .get(self.pos[self.category])
            .copied()
    }

    /// Keep the hovered index inside the category's item range after a
    /// category switch.
    fn clamp_item(&mut self) {
        let n = items_in(self.category).len().saturating_sub(1);
        self.pos[self.category] = self.pos[self.category].min(n);
    }

    pub fn key_scope(&self) -> crate::key_mapping::Scope {
        use crate::key_mapping::Scope;
        match self.editing {
            Some(Edit::Input { .. }) => Scope::PageEdit,
            Some(Edit::Choice { .. }) => Scope::PageChoice,
            None => Scope::Page,
        }
    }

    #[cfg(test)]
    pub fn handle_key(&mut self, key: &KeyEvent, config: &mut Config) -> SettingsAction {
        self.handle_input(config.key_mapping.input(self.key_scope(), key), config)
    }

    pub fn handle_input(
        &mut self,
        key: crate::key_mapping::MappedKey,
        config: &mut Config,
    ) -> SettingsAction {
        use crate::key_mapping::{Action, MappedKey::Command};
        if let Some(edit) = self.editing.take() {
            let def = self.current_item();
            let options: Vec<String> = def
                .map(|d| dynamic_options(d, config, &self.modes, &self.themes))
                .unwrap_or_default();
            match edit {
                Edit::Input { buf } => {
                    let mut editor = TextEditor { buf, secret: false };
                    match handle_text_input(&mut editor, key) {
                        TextEditResult::Confirm(value) => {
                            if let Some(def) = def {
                                (def.apply)(config, value);
                                return SettingsAction::Changed;
                            }
                        }
                        TextEditResult::Cancel => {}
                        TextEditResult::Continue => {
                            self.editing = Some(Edit::Input { buf: editor.buf });
                        }
                    }
                }
                Edit::Choice { cursor } => {
                    let n = options.len().max(1);
                    match key {
                        Command(Action::Confirm) => {
                            if let (Some(def), Some(opt)) = (def, options.get(cursor)) {
                                (def.apply)(config, opt.clone());
                                return SettingsAction::Changed;
                            }
                        }
                        Command(Action::Cancel) => {}
                        Command(Action::Previous) => {
                            self.editing = Some(Edit::Choice {
                                cursor: (cursor + n - 1) % n,
                            });
                        }
                        Command(Action::Next) => {
                            self.editing = Some(Edit::Choice {
                                cursor: (cursor + 1) % n,
                            });
                        }
                        _ => self.editing = Some(Edit::Choice { cursor }),
                    }
                }
            }
            return SettingsAction::None;
        }

        // ---- browsing: category tabs stay outside the focus graph.
        match key {
            Command(Action::Back | Action::Close) => SettingsAction::Exit,
            Command(Action::MoveLeft) => {
                self.category = (self.category + CATEGORIES.len() - 1) % CATEGORIES.len();
                self.clamp_item();
                self.scroll = 0;
                SettingsAction::None
            }
            Command(Action::MoveRight) => {
                self.category = (self.category + 1) % CATEGORIES.len();
                self.clamp_item();
                self.scroll = 0;
                SettingsAction::None
            }
            Command(Action::MoveUp) => {
                let items = items_in(self.category);
                let current = self.pos[self.category];
                if let Some(previous) = (0..current)
                    .rev()
                    .find(|index| items[*index].kind != ItemKind::ReadOnly)
                {
                    self.pos[self.category] = previous;
                }
                SettingsAction::None
            }
            Command(Action::MoveDown) => {
                let items = items_in(self.category);
                let current = self.pos[self.category];
                if let Some(next) = (current + 1..items.len())
                    .find(|index| items[*index].kind != ItemKind::ReadOnly)
                {
                    self.pos[self.category] = next;
                }
                SettingsAction::None
            }
            Command(Action::Confirm) => {
                if let Some(def) = self.current_item() {
                    match def.kind {
                        ItemKind::Choice { options } => {
                            let current = (def.get)(config);
                            let cursor = options
                                .iter()
                                .position(|(value, _)| *value == current)
                                .unwrap_or(0);
                            self.editing = Some(Edit::Choice { cursor });
                        }
                        ItemKind::ModeChoice => {
                            let current = (def.get)(config);
                            let cursor = dynamic_options(def, config, &self.modes, &self.themes)
                                .iter()
                                .position(|mode| *mode == current)
                                .unwrap_or(0);
                            self.editing = Some(Edit::Choice { cursor });
                        }
                        ItemKind::ThemeChoice => {
                            let current = (def.get)(config);
                            let cursor = dynamic_options(def, config, &self.modes, &self.themes)
                                .iter()
                                .position(|theme| *theme == current)
                                .unwrap_or(0);
                            self.editing = Some(Edit::Choice { cursor });
                        }
                        ItemKind::Input => {
                            self.editing = Some(Edit::Input { buf: String::new() });
                        }
                        ItemKind::ReadOnly => {}
                    }
                }
                SettingsAction::None
            }
            _ => SettingsAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn left_right_and_hl_switch_categories_directly() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.scroll = 4;

        s.handle_key(&key(KeyCode::Left), &mut config);
        assert_eq!(s.category, 3, "Left wraps to the last page");
        assert_eq!(s.scroll, 0);
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.category, 0);
        s.handle_key(&key(KeyCode::Char('l')), &mut config);
        assert_eq!(s.category, 1);
        s.handle_key(&key(KeyCode::Char('h')), &mut config);
        assert_eq!(s.category, 0);
    }

    #[test]
    fn jk_and_arrows_move_items() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        assert_eq!(s.pos[0], 0);
        s.handle_key(&key(KeyCode::Char('j')), &mut config);
        assert_eq!(s.pos[0], 1);
        s.handle_key(&key(KeyCode::Down), &mut config);
        assert_eq!(s.pos[0], 2);
        s.handle_key(&key(KeyCode::Char('k')), &mut config);
        assert_eq!(s.pos[0], 1);
        s.handle_key(&key(KeyCode::Up), &mut config);
        assert_eq!(s.pos[0], 0);
        // Ends clamp instead of wrapping.
        s.handle_key(&key(KeyCode::Up), &mut config);
        assert_eq!(s.pos[0], 0);
        s.pos[0] = items_in(0).len() - 1;
        s.handle_key(&key(KeyCode::Down), &mut config);
        assert_eq!(s.pos[0], items_in(0).len() - 1);
    }

    #[test]
    fn category_switch_retains_or_clamps_remembered_item() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.pos[1] = 3;
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.category, 1);
        assert_eq!(s.pos[1], 3, "remembered position is retained");

        s.category = 2;
        s.pos[3] = 5;
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.category, 3);
        assert_eq!(s.pos[3], 0, "read-only page still clamps its row index");
    }

    #[test]
    fn enter_edits_choice_confirm_and_esc_cancel() {
        let mut s = SettingsState::default();
        s.themes = vec!["dracula".into(), "ferra".into()];
        let mut config = Config::default();
        config.theme = "dracula".into();
        assert_eq!(config.theme, "dracula");
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(
            s.editing,
            Some(Edit::Choice { cursor: 0 }),
            "cursor on the current value"
        );
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 1 }));
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.theme, "ferra", "Enter confirms the choice");
        // Esc cancels the next edit (nothing applied).
        s.handle_key(&key(KeyCode::Enter), &mut config);
        s.handle_key(&key(KeyCode::Left), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 0 }));
        s.handle_key(&key(KeyCode::Esc), &mut config);
        assert_eq!(s.editing, None);
        assert_eq!(config.theme, "ferra", "cancelled edit keeps the value");
    }

    #[test]
    fn bool_edits_as_two_option_choice() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.pos[0] = 1; // 纯色模式
        assert_eq!((ITEMS[1].get)(&config), "off");
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 1 }), "cursor on 关");
        s.handle_key(&key(KeyCode::Right), &mut config); // wraps to 开
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(config.plain_color);
    }

    #[test]
    fn enter_edits_number_confirm_and_cancel() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.pos[0] = 3; // 消息/输入框内边距
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(matches!(s.editing, Some(Edit::Input { .. })));
        for c in "6".chars() {
            s.handle_key(&key(KeyCode::Char(c)), &mut config);
        }
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.user_input_padding, 6);
        // Esc cancels the next edit.
        s.handle_key(&key(KeyCode::Enter), &mut config);
        for c in "99".chars() {
            s.handle_key(&key(KeyCode::Char(c)), &mut config);
        }
        s.handle_key(&key(KeyCode::Esc), &mut config);
        assert_eq!(
            config.user_input_padding, 6,
            "cancelled edit keeps the value"
        );
    }

    #[test]
    fn reveal_inputs_apply_valid_values_and_retain_old_values_when_invalid() {
        fn confirm(
            state: &mut SettingsState,
            config: &mut Config,
            category: usize,
            setting_key: &str,
            value: &str,
        ) {
            state.category = category;
            state.pos[category] = items_in(category)
                .iter()
                .position(|item| item.key == setting_key)
                .expect("setting exists");
            state.handle_key(&key(KeyCode::Enter), config);
            for character in value.chars() {
                state.handle_key(&key(KeyCode::Char(character)), config);
            }
            assert!(matches!(
                state.handle_key(&key(KeyCode::Enter), config),
                SettingsAction::Changed
            ));
        }

        let mut state = SettingsState::default();
        let mut config = Config::default();
        confirm(&mut state, &mut config, 0, "background_color", "#1A2b3C");
        assert_eq!(config.background_color.to_string(), "#1a2b3c");
        confirm(&mut state, &mut config, 0, "background_color", "black");
        assert_eq!(config.background_color.to_string(), "#1a2b3c");
        confirm(&mut state, &mut config, 0, "message_pane_percent", "61.25");
        assert_eq!(config.message_pane_percent.display(), "61.25%");
        confirm(&mut state, &mut config, 0, "message_pane_percent", "24.99");
        assert_eq!(config.message_pane_percent.display(), "61.25%");

        confirm(
            &mut state,
            &mut config,
            2,
            "message_chars_per_second",
            "1024",
        );
        assert_eq!(config.message_chars_per_second.get(), 1024);
        confirm(&mut state, &mut config, 2, "message_chars_per_second", "0");
        assert_eq!(config.message_chars_per_second.get(), 0);
        confirm(&mut state, &mut config, 2, "preview_lines_per_second", "7");
        assert_eq!(config.preview_lines_per_second.get(), 7);
        confirm(
            &mut state,
            &mut config,
            2,
            "preview_lines_per_second",
            "1025",
        );
        assert_eq!(config.preview_lines_per_second.get(), 7);
    }

    #[test]
    fn page_align_choice_edits() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        assert_eq!(config.page_align, "center");
        assert_eq!(config.page_align_value(), "center");
        let index = items_in(0)
            .iter()
            .position(|item| item.key == "page_align")
            .expect("页面对齐 item exists");
        s.pos[0] = index;
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 0 }));
        s.handle_key(&key(KeyCode::Right), &mut config); // 左对齐
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.page_align, "left");
        // The next edit reopens on the current value and wraps to 右对齐.
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 1 }));
        s.handle_key(&key(KeyCode::Right), &mut config); // 右对齐
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.page_align, "right");
        assert_eq!(config.page_align_value(), "right");
    }

    #[test]
    fn esc_exits() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        assert!(matches!(
            s.handle_key(&key(KeyCode::Esc), &mut config),
            SettingsAction::Exit
        ));
    }

    #[test]
    fn mode_choice_edits_over_the_roster() {
        let mut s = SettingsState::default();
        s.modes = vec!["standard".into(), "minimal".into(), "cordis".into()];
        s.category = 1; // 行为
        let mut config = Config::default();
        assert_eq!(config.default_mode, "standard");
        s.pos[1] = 2; // 默认模式 (行为 category, third item after Language)
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(
            s.editing,
            Some(Edit::Choice { cursor: 0 }),
            "cursor on the current mode"
        );
        s.handle_key(&key(KeyCode::Right), &mut config); // minimal
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.default_mode, "minimal", "Enter confirms the mode");
    }

    #[test]
    fn thinking_display_choice_edits_and_line_budget_input() {
        let mut s = SettingsState::default();
        s.category = 2; // 显示
        let mut config = Config::default();
        assert_eq!(config.thinking_display, "compact");
        assert_eq!(config.thinking_lines, 2);

        let mode_index = items_in(2)
            .iter()
            .position(|item| item.key == "thinking_display")
            .expect("Thinking mode item exists");
        s.pos[2] = mode_index;
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 0 }));
        s.handle_key(&key(KeyCode::Right), &mut config); // Lines
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.thinking_display, "lines");

        let lines_index = items_in(2)
            .iter()
            .position(|item| item.key == "thinking_lines")
            .expect("Thinking lines item exists");
        s.pos[2] = lines_index;
        s.handle_key(&key(KeyCode::Enter), &mut config);
        for c in "5".chars() {
            s.handle_key(&key(KeyCode::Char(c)), &mut config);
        }
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.thinking_lines, 5);
    }

    #[test]
    fn stale_default_mode_stays_selectable() {
        let mut config = Config::default();
        config.default_mode = "gone".into();
        let modes = vec!["standard".to_string(), "minimal".to_string()];
        let no_themes: Vec<String> = vec![];
        let mode_def = ITEMS
            .iter()
            .find(|item| item.key == "default_mode")
            .expect("默认模式 item exists");
        let opts = dynamic_options(mode_def, &config, &modes, &no_themes);
        assert_eq!(opts, vec!["standard", "minimal", "gone"]);
        // Static choices pass through unchanged.
        assert_eq!(
            dynamic_options(&ITEMS[1], &config, &modes, &no_themes),
            vec!["on", "off"]
        );
    }

    #[test]
    fn stale_theme_stays_selectable() {
        let mut config = Config::default();
        config.theme = "custom-mine".into();
        let themes = vec!["dracula".to_string(), "ferra".to_string()];
        let opts = dynamic_options(&ITEMS[0], &config, &[], &themes);
        assert_eq!(opts, vec!["dracula", "ferra", "custom-mine"]);
    }
}
