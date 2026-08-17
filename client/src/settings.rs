//! /settings Input Page: category tabs and editable rows share one visible
//! focus. The page replaces the ordinary input area without a floating border.

use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    config::Config,
    input_page::{handle_text_editor, TextEditResult, TextEditor},
};

pub const CATEGORIES: &[&str] = &["外观", "行为", "显示", "高级"];

/// Value kinds. Booleans are just two-option choices (开/关).
#[derive(Clone, Copy, PartialEq)]
pub enum ItemKind {
    Choice {
        options: &'static [&'static str],
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
    pub label: &'static str,
    /// One-sentence description; rendered dark under the label.
    pub desc: &'static str,
    pub kind: ItemKind,
    /// Read the current value as a display string (an option label for
    /// choices, the number for inputs).
    pub get: fn(&Config) -> String,
    /// Apply a confirmed value (an option label or the typed number).
    pub apply: fn(&mut Config, value: String),
}

pub static ITEMS: &[ItemDef] = &[
    ItemDef {
        category: 0,
        label: "主题",
        desc: "配色主题（themes 目录下的合法主题；默认 deepseek-e）",
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
        label: "纯色模式",
        desc: "降级为纯色输出（NO_COLOR 语义）",
        kind: ItemKind::Choice {
            options: &["开", "关"],
        },
        get: |c| bool_str(c.plain_color),
        apply: |c, v| {
            c.plain_color = v == "开";
        },
    },
    ItemDef {
        category: 0,
        label: "消息/输入框内边距",
        desc: "用户消息块与输入栏的水平内边距（空格数）",
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
        label: "页面最大宽度",
        desc: "正文列最大宽度（列），0 表示不限，内容居中",
        kind: ItemKind::Input,
        get: |c| c.page_max_width.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse::<usize>() {
                c.page_max_width = n.min(500);
            }
        },
    },
    ItemDef {
        category: 1,
        label: "记住上次会话",
        desc: "启动时自动续接上次会话（默认关：新进程开新会话）",
        kind: ItemKind::Choice {
            options: &["开", "关"],
        },
        get: |c| bool_str(c.remember_last_session),
        apply: |c, v| {
            c.remember_last_session = v == "开";
        },
    },
    ItemDef {
        category: 1,
        label: "默认模式",
        desc: "新开 TUI 进程创建会话时使用的模式（失效时回退标准模式）",
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
        label: "粘贴占位阈值",
        desc: "粘贴内容超过该字符数时折叠为原子粘贴块（光标不可进入）",
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
        label: "原子块折叠阈值",
        desc: "表格/代码/mermaid 超过该行数时折叠",
        kind: ItemKind::Input,
        get: |c| c.atomic_collapse_rows.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse() {
                c.atomic_collapse_rows = n;
            }
        },
    },
    ItemDef {
        category: 1,
        label: "复制提示时长",
        desc: "复制成功的提示停留时间（秒）",
        kind: ItemKind::Input,
        get: |c| c.copy_toast_secs.to_string(),
        apply: |c, v| {
            if let Ok(n) = v.parse() {
                c.copy_toast_secs = n;
            }
        },
    },
    ItemDef {
        category: 1,
        label: "历史条数",
        desc: "输入历史保留条数",
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
        label: "工具耗时显示",
        desc: "工具卡完成时显示执行耗时",
        kind: ItemKind::Choice {
            options: &["开", "关"],
        },
        get: |c| bool_str(c.show_tool_duration),
        apply: |c, v| {
            c.show_tool_duration = v == "开";
        },
    },
    ItemDef {
        category: 2,
        label: "read/edit 合并为一行",
        desc: "连续 read/edit 调用折叠为一行",
        kind: ItemKind::Choice {
            options: &["开", "关"],
        },
        get: |c| bool_str(c.read_merge),
        apply: |c, v| {
            c.read_merge = v == "开";
        },
    },
    ItemDef {
        category: 2,
        label: "mermaid 渲染",
        desc: "mermaid 代码块渲染为框图（失败时显示源码）",
        kind: ItemKind::Choice {
            options: &["开", "关"],
        },
        get: |c| bool_str(c.mermaid_enabled),
        apply: |c, v| {
            c.mermaid_enabled = v == "开";
        },
    },
    ItemDef {
        category: 3,
        label: "配置位置",
        desc: "配置文件完整路径（只读）",
        kind: ItemKind::ReadOnly,
        get: |_| Config::config_path().display().to_string(),
        apply: |_, _| {},
    },
];

fn bool_str(b: bool) -> String {
    if b {
        "开".into()
    } else {
        "关".into()
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
        ItemKind::Choice { options } => options.iter().map(|o| (*o).to_string()).collect(),
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
    /// Current category page.
    pub category: usize,
    /// The category tab currently under the single page focus.
    pub tab_cursor: usize,
    /// True while the focus is on the category row; otherwise it is on an
    /// actionable item in the active category.
    pub focus_tabs: bool,
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
            tab_cursor: 0,
            focus_tabs: false,
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

    pub fn handle_key(&mut self, key: &KeyEvent, config: &mut Config) -> SettingsAction {
        // ---- editing: Enter confirms, Esc cancels, everything else is
        // ---- consumed by the edit (←/→ move the choice cursor).
        if let Some(edit) = self.editing.take() {
            let def = self.current_item();
            let options: Vec<String> = def
                .map(|d| dynamic_options(d, config, &self.modes, &self.themes))
                .unwrap_or_default();
            match edit {
                Edit::Input { buf } => {
                    let mut editor = TextEditor { buf, secret: false };
                    match handle_text_editor(&mut editor, key) {
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
                    match key.code {
                        KeyCode::Enter => {
                            if let (Some(def), Some(opt)) = (def, options.get(cursor)) {
                                (def.apply)(config, opt.clone());
                                return SettingsAction::Changed;
                            }
                        }
                        KeyCode::Esc => {}
                        KeyCode::Left | KeyCode::Char('h') => {
                            self.editing = Some(Edit::Choice {
                                cursor: (cursor + n - 1) % n,
                            });
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
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

        // ---- browsing: one focus covers the tabs and actionable rows.
        let items = items_in(self.category);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => SettingsAction::Exit,
            KeyCode::Left | KeyCode::Char('h') if self.focus_tabs => {
                self.tab_cursor = (self.tab_cursor + CATEGORIES.len() - 1) % CATEGORIES.len();
                SettingsAction::None
            }
            KeyCode::Right | KeyCode::Char('l') if self.focus_tabs => {
                self.tab_cursor = (self.tab_cursor + 1) % CATEGORIES.len();
                SettingsAction::None
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
                self.focus_tabs = true;
                self.tab_cursor = self.category;
                SettingsAction::None
            }
            KeyCode::Up | KeyCode::Char('k') if self.focus_tabs => SettingsAction::None,
            KeyCode::Down | KeyCode::Char('j') if self.focus_tabs => {
                if items.iter().any(|item| item.kind != ItemKind::ReadOnly) {
                    self.focus_tabs = false;
                    self.clamp_item();
                }
                SettingsAction::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let current = self.pos[self.category];
                if let Some(previous) = (0..current)
                    .rev()
                    .find(|index| items[*index].kind != ItemKind::ReadOnly)
                {
                    self.pos[self.category] = previous;
                } else {
                    self.focus_tabs = true;
                    self.tab_cursor = self.category;
                }
                SettingsAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let current = self.pos[self.category];
                if let Some(next) = (current + 1..items.len())
                    .find(|index| items[*index].kind != ItemKind::ReadOnly)
                {
                    self.pos[self.category] = next;
                }
                SettingsAction::None
            }
            KeyCode::Enter if self.focus_tabs => {
                self.category = self.tab_cursor;
                self.clamp_item();
                self.scroll = 0;
                if items_in(self.category)
                    .iter()
                    .any(|item| item.kind != ItemKind::ReadOnly)
                {
                    self.focus_tabs = false;
                }
                SettingsAction::None
            }
            KeyCode::Enter => {
                if let Some(def) = self.current_item() {
                    match def.kind {
                        ItemKind::Choice { options } => {
                            let current = (def.get)(config);
                            let cursor = options
                                .iter()
                                .position(|o| (*o).starts_with(current.as_str()))
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
    fn tabs_are_focusable_and_enter_activates_one() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.handle_key(&key(KeyCode::Left), &mut config);
        assert!(s.focus_tabs);
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.tab_cursor, 1);
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.category, 1);
        assert!(!s.focus_tabs, "actionable category enters its item list");
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
    fn category_activation_clamps_hover_and_skips_read_only_page() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.pos[1] = 3;
        s.focus_tabs = true;
        s.tab_cursor = 1;
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.category, 1);
        assert_eq!(s.pos[1], 3, "remembered position is retained");

        s.focus_tabs = true;
        s.tab_cursor = 3;
        s.pos[3] = 5;
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.category, 3);
        assert_eq!(s.pos[3], 0);
        assert!(s.focus_tabs, "read-only rows never receive focus");
    }

    #[test]
    fn enter_edits_choice_confirm_and_esc_cancel() {
        let mut s = SettingsState::default(); // 主题: deepseek-e/ferra
        s.themes = vec!["deepseek-e".into(), "ferra".into()];
        let mut config = Config::default();
        assert_eq!(config.theme, "deepseek-e");
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
        assert_eq!((ITEMS[1].get)(&config), "关");
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
        s.pos[0] = 2; // 消息/输入框内边距
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
        s.pos[1] = 1; // 默认模式 (行为 category, second item)
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
    fn stale_default_mode_stays_selectable() {
        let mut config = Config::default();
        config.default_mode = "gone".into();
        let modes = vec!["standard".to_string(), "minimal".to_string()];
        let no_themes: Vec<String> = vec![];
        let opts = dynamic_options(&ITEMS[5], &config, &modes, &no_themes);
        assert_eq!(opts, vec!["standard", "minimal", "gone"]);
        // Static choices pass through unchanged.
        assert_eq!(
            dynamic_options(&ITEMS[1], &config, &modes, &no_themes),
            vec!["开", "关"]
        );
    }

    #[test]
    fn stale_theme_stays_selectable() {
        let mut config = Config::default();
        config.theme = "custom-mine".into();
        let themes = vec!["deepseek-e".to_string(), "ferra".to_string()];
        let opts = dynamic_options(&ITEMS[0], &config, &[], &themes);
        assert_eq!(opts, vec!["deepseek-e", "ferra", "custom-mine"]);
    }
}
