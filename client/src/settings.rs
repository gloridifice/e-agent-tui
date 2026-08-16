//! /settings panel (design §4.7, D26–D30): replaces the input bar and the
//! rows above it — no floating window, no border. ←/→ (h/l) switch category
//! pages (the centered tab row itself is not selectable), ↑/↓ (j/k) move
//! between items. Enter edits the hovered item's value, Enter confirms,
//! Esc cancels — or exits the panel when not editing.

use crossterm::event::{KeyCode, KeyEvent};

use crate::config::Config;

pub const CATEGORIES: &[&str] = &["外观", "行为", "显示", "高级"];

/// Value kinds. Booleans are just two-option choices (开/关).
#[derive(Clone, Copy, PartialEq)]
pub enum ItemKind {
    Choice { options: &'static [&'static str] },
    /// Choice over the live agent-preset roster (`/new` modes): the options
    /// are not static — they come from `SettingsState.modes`, fed by the
    /// bridge's `presets` message.
    ModeChoice,
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
        desc: "ferra 预设或自定义色板（自定义色板在 TOML 中手改）",
        kind: ItemKind::Choice { options: &["ferra", "custom"] },
        get: |c| c.theme.preset.clone(),
        apply: |c, v| {
            c.theme.preset = v;
        },
    },
    ItemDef {
        category: 0,
        label: "纯色模式",
        desc: "降级为纯色输出（NO_COLOR 语义）",
        kind: ItemKind::Choice { options: &["开", "关"] },
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
        kind: ItemKind::Choice { options: &["开", "关"] },
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
        label: "发送键风格",
        desc: "单行模式下 Enter 直接发送，或改由 Ctrl+Enter 发送",
        kind: ItemKind::Choice { options: &["Enter 即发", "Ctrl+Enter 发送"] },
        get: |c| {
            if c.enter_sends {
                "Enter 即发".into()
            } else {
                "Ctrl+Enter 发送".into()
            }
        },
        apply: |c, v| {
            c.enter_sends = v == "Enter 即发";
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
        label: "状态栏模型名",
        desc: "状态栏显示 provider · model",
        kind: ItemKind::Choice { options: &["开", "关"] },
        get: |c| bool_str(c.show_model_in_status),
        apply: |c, v| {
            c.show_model_in_status = v == "开";
        },
    },
    ItemDef {
        category: 2,
        label: "工具耗时显示",
        desc: "工具卡完成时显示执行耗时",
        kind: ItemKind::Choice { options: &["开", "关"] },
        get: |c| bool_str(c.show_tool_duration),
        apply: |c, v| {
            c.show_tool_duration = v == "开";
        },
    },
    ItemDef {
        category: 2,
        label: "read/edit 合并为一行",
        desc: "连续 read/edit 调用折叠为一行",
        kind: ItemKind::Choice { options: &["开", "关"] },
        get: |c| bool_str(c.read_merge),
        apply: |c, v| {
            c.read_merge = v == "开";
        },
    },
    ItemDef {
        category: 2,
        label: "mermaid 渲染",
        desc: "mermaid 代码块渲染为框图（失败时显示源码）",
        kind: ItemKind::Choice { options: &["开", "关"] },
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

/// Option list of a choice/mode-choice item: the static labels, or the live
/// mode roster (`SettingsState.modes`, the ids of the presets the bridge
/// sent). The current value is appended when the roster lacks it, so a
/// configured mode that no longer exists stays visible (the bridge falls
/// back to `standard` on the next fresh process anyway).
pub fn dynamic_options(def: &ItemDef, config: &Config, modes: &[String]) -> Vec<String> {
    match def.kind {
        ItemKind::Choice { options } => options.iter().map(|o| (*o).to_string()).collect(),
        ItemKind::ModeChoice => {
            let mut list: Vec<String> = modes.to_vec();
            if !list.iter().any(|m| *m == config.default_mode) {
                list.push(config.default_mode.clone());
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
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            category: 0,
            pos: [0; 4],
            editing: None,
            scroll: 0,
            modes: Vec::new(),
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
        items_in(self.category).get(self.pos[self.category]).copied()
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
                .map(|d| dynamic_options(d, config, &self.modes))
                .unwrap_or_default();
            match edit {
                Edit::Input { buf } => match key.code {
                    KeyCode::Enter => {
                        if let Some(def) = def {
                            (def.apply)(config, buf);
                            return SettingsAction::Changed;
                        }
                    }
                    KeyCode::Esc => {}
                    KeyCode::Char(c) if !c.is_ascii_control() => {
                        let mut next = buf;
                        next.push(c);
                        self.editing = Some(Edit::Input { buf: next });
                    }
                    KeyCode::Backspace => {
                        let mut next = buf;
                        next.pop();
                        self.editing = Some(Edit::Input { buf: next });
                    }
                    _ => self.editing = Some(Edit::Input { buf }),
                },
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
                            self.editing = Some(Edit::Choice { cursor: (cursor + n - 1) % n });
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            self.editing = Some(Edit::Choice { cursor: (cursor + 1) % n });
                        }
                        _ => self.editing = Some(Edit::Choice { cursor }),
                    }
                }
            }
            return SettingsAction::None;
        }

        // ---- browsing: ←/→ (h/l) switch category pages, ↑/↓ (j/k) move
        // ---- between items.
        let n = items_in(self.category).len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => SettingsAction::Exit,
            KeyCode::Left | KeyCode::Char('h') => {
                self.category = (self.category + CATEGORIES.len() - 1) % CATEGORIES.len();
                self.clamp_item();
                self.scroll = 0;
                SettingsAction::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.category = (self.category + 1) % CATEGORIES.len();
                self.clamp_item();
                self.scroll = 0;
                SettingsAction::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.pos[self.category] > 0 {
                    self.pos[self.category] -= 1;
                }
                SettingsAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 && self.pos[self.category] + 1 < n {
                    self.pos[self.category] += 1;
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
                            // Cursor on the configured mode (present even
                            // when the roster no longer lists it — see
                            // `dynamic_options`).
                            let current = (def.get)(config);
                            let cursor = self
                                .modes
                                .iter()
                                .position(|m| *m == current)
                                .unwrap_or(0);
                            self.editing = Some(Edit::Choice { cursor });
                        }
                        ItemKind::Input => {
                            // Replace semantics: start with an empty buffer.
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
    fn hl_and_arrows_switch_categories() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.category, 1);
        s.handle_key(&key(KeyCode::Char('h')), &mut config);
        assert_eq!(s.category, 0);
        s.handle_key(&key(KeyCode::Char('l')), &mut config);
        assert_eq!(s.category, 1);
        s.handle_key(&key(KeyCode::Left), &mut config);
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
    fn category_switch_clamps_hover() {
        let mut s = SettingsState::default();
        let mut config = Config::default();
        s.pos[1] = 3;
        s.handle_key(&key(KeyCode::Right), &mut config); // → 行为 (6 items)
        assert_eq!(s.pos[1], 3, "position kept when it fits");
        // A remembered position beyond the new page's range is clamped.
        s.pos[3] = 5; // 高级 has 1 item
        s.handle_key(&key(KeyCode::Right), &mut config);
        s.handle_key(&key(KeyCode::Right), &mut config); // → 高级
        assert_eq!(s.pos[3], 0, "position clamped to the last item");
    }

    #[test]
    fn enter_edits_choice_confirm_and_esc_cancel() {
        let mut s = SettingsState::default(); // 主题: ferra/custom
        let mut config = Config::default();
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 0 }), "cursor on the current value");
        s.handle_key(&key(KeyCode::Right), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 1 }));
        s.handle_key(&key(KeyCode::Enter), &mut config);
        assert_eq!(config.theme.preset, "custom", "Enter confirms the choice");
        // Esc cancels the next edit (nothing applied).
        s.handle_key(&key(KeyCode::Enter), &mut config);
        s.handle_key(&key(KeyCode::Left), &mut config);
        assert_eq!(s.editing, Some(Edit::Choice { cursor: 0 }));
        s.handle_key(&key(KeyCode::Esc), &mut config);
        assert_eq!(s.editing, None);
        assert_eq!(config.theme.preset, "custom", "cancelled edit keeps the value");
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
        assert_eq!(config.user_input_padding, 6, "cancelled edit keeps the value");
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
        let opts = dynamic_options(&ITEMS[5], &config, &modes);
        assert_eq!(opts, vec!["standard", "minimal", "gone"]);
        // Static choices pass through unchanged.
        assert_eq!(dynamic_options(&ITEMS[1], &config, &modes), vec!["开", "关"]);
    }
}
