//! Client configuration (design §4.7, D25–D30): persisted TOML in
//! %APPDATA%\dsh-tui\config.toml, editable live through /settings.
//! Defaults are the documented design values; the Theme derives from the
//! configured palette.

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

// ---------- theme (D5, ferra palette) ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ThemeConfig {
    /// "ferra" preset or "custom".
    pub preset: String,
    /// Hex overrides for custom presets ("bg", "fg", "user", …).
    pub overrides: HashMap<String, String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self { preset: "ferra".into(), overrides: HashMap::new() }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub bg_soft: Color,
    pub selection: Color,
    pub dim: Color,
    pub fg: Color,
    pub ok: Color,
    pub link: Color,
    pub user: Color,
    pub rose: Color,
    pub err: Color,
    pub running: Color,
}

impl Theme {
    pub fn ferra() -> Self {
        Self {
            bg: Color::Rgb(0x2b, 0x29, 0x2d),
            bg_soft: Color::Rgb(0x38, 0x35, 0x39),
            selection: Color::Rgb(0x4d, 0x42, 0x4b),
            dim: Color::Rgb(0x6f, 0x5d, 0x63),
            fg: Color::Rgb(0xd1, 0xd1, 0xe0),
            ok: Color::Rgb(0xb1, 0xb6, 0x95),
            link: Color::Rgb(0xfe, 0xcd, 0xb2),
            user: Color::Rgb(0xff, 0xa0, 0x7a),
            rose: Color::Rgb(0xf6, 0xb6, 0xc9),
            err: Color::Rgb(0xe0, 0x6b, 0x75),
            running: Color::Rgb(0xf5, 0xd7, 0x6e),
        }
    }

    pub fn from_config(cfg: &ThemeConfig) -> Self {
        if cfg.preset != "custom" && cfg.overrides.is_empty() {
            return Self::ferra();
        }
        let base = Self::ferra();
        let apply = |field: &mut Color, key: &str| {
            if let Some(hex) = cfg.overrides.get(key) {
                if let Some(c) = parse_hex(hex) {
                    *field = c;
                }
            }
        };
        let mut t = base;
        apply(&mut t.bg, "bg");
        apply(&mut t.bg_soft, "bg_soft");
        apply(&mut t.selection, "selection");
        apply(&mut t.dim, "dim");
        apply(&mut t.fg, "fg");
        apply(&mut t.ok, "ok");
        apply(&mut t.link, "link");
        apply(&mut t.user, "user");
        apply(&mut t.rose, "rose");
        apply(&mut t.err, "err");
        apply(&mut t.running, "running");
        t
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

// ---------- full config (D28) ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    // 外观
    pub spinner_style: String,
    pub spinner_frame_ms: u64,
    pub theme: ThemeConfig,
    pub plain_color: bool,
    // 行为
    pub remember_last_session: bool,
    /// Agent-preset mode for the session a fresh TUI process opens (the
    /// bridge falls back to `standard` when this id is stale).
    pub default_mode: String,
    pub enter_sends: bool,
    pub paste_placeholder_chars: usize,
    pub long_content_lines: usize,
    pub atomic_collapse_rows: usize,
    pub copy_toast_secs: u64,
    pub history_limit: usize,
    // 显示
    pub show_model_in_status: bool,
    pub show_tool_duration: bool,
    pub read_merge: bool,
    pub show_timestamps: bool,
    pub mermaid_enabled: bool,
    /// Horizontal gutter (in columns) of user message blocks and the input
    /// box — live-editable via /settings.
    pub user_input_padding: usize,
    /// Maximum page width in columns (0 = unlimited, use the terminal width
    /// minus the side margins). The content area is centered and capped at
    /// this width; longer text wraps.
    pub page_max_width: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            spinner_style: "A".into(),
            spinner_frame_ms: 120,
            theme: ThemeConfig::default(),
            plain_color: false,
            // A fresh TUI process opens a NEW session by default; resume
            // goes through the CLI session id, `/resume`, or this opt-in.
            remember_last_session: false,
            default_mode: "standard".into(),
            enter_sends: true,
            paste_placeholder_chars: 64,
            long_content_lines: 20,
            atomic_collapse_rows: 40,
            copy_toast_secs: 2,
            history_limit: 1000,
            show_model_in_status: true,
            show_tool_duration: true,
            read_merge: true,
            show_timestamps: false,
            mermaid_enabled: true,
            user_input_padding: 2,
            page_max_width: 0,
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "dsh-tui")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("dsh-tui.toml"))
    }

    pub fn state_path() -> PathBuf {
        directories::ProjectDirs::from("", "", "dsh-tui")
            .map(|d| d.data_dir().join("state.toml"))
            .unwrap_or_else(|| PathBuf::from("dsh-tui.state.toml"))
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|error| {
                eprintln!("[dsh-tui] config parse failed ({error}); using defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    pub fn theme(&self) -> Theme {
        Theme::from_config(&self.theme)
    }
}

/// Last-attached session id, remembered across runs (D17).
#[derive(Serialize, Deserialize, Default)]
pub struct StateFile {
    pub last_session_id: Option<String>,
}

impl StateFile {
    pub fn load() -> Self {
        match std::fs::read_to_string(Config::state_path()) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Config::state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
