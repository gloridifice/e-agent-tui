//! /login panel (D33): the input bar becomes a login settings page with
//! three fields — API key (host credentials seam, value never read back),
//! 账号 (the harness anonymous user id sent as x-deepseek-harness-user-id),
//! and proxy (the HTTPS_PROXY line of the harness-home .env, applied on the
//! next host restart). ↑/↓ move, Enter edits, Esc exits — every confirmed
//! edit is sent to the bridge immediately (即改即存).
//!
//! The panel mirrors the /settings panel's shape (borderless, replaces the
//! input bar) but its data lives on the bridge, not in config.toml: the
//! client requests it with `login-get` and the `login` frame refreshes it.

use crossterm::event::{KeyCode, KeyEvent};

/// The three editable fields, in display order.
pub const FIELDS: &[(&str, &str)] = &[
    ("apiKey", "API key"),
    ("account", "账号"),
    ("proxy", "proxy"),
];

/// What the UI should do after a key press.
#[derive(Debug, PartialEq)]
pub enum LoginAction {
    None,
    Exit,
    /// A field edit was confirmed — send it to the bridge.
    Set { field: &'static str, value: String },
}

/// In-progress edit of the focused value. Enter confirms, Esc cancels.
#[derive(Debug, PartialEq)]
pub enum Edit {
    /// The typed buffer (apiKey starts empty — the secret is never
    /// pre-filled; account/proxy start with their current value).
    Input { buf: String },
}

pub struct LoginState {
    /// Hovered field index (0..FIELDS.len()).
    pub pos: usize,
    /// Active edit.
    pub editing: Option<Edit>,
    // ---- bridge-synced state (the `login` frame) ----
    pub api_key_configured: bool,
    pub api_key_writable: bool,
    pub api_key_source: Option<String>,
    pub api_key_hint: Option<String>,
    pub account: Option<String>,
    pub proxy: Option<String>,
    /// Message of the last rejected write; cleared on the next success.
    pub error: Option<String>,
    /// The initial login-get has not been answered yet.
    pub loading: bool,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            pos: 0,
            editing: None,
            api_key_configured: false,
            api_key_writable: false,
            api_key_source: None,
            api_key_hint: None,
            account: None,
            proxy: None,
            error: None,
            loading: true,
        }
    }
}

impl LoginState {
    /// Apply one bridge `login` frame (via crate::protocol::ServerMessage::Login).
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        api_key_configured: bool,
        api_key_writable: bool,
        api_key_source: Option<String>,
        api_key_hint: Option<String>,
        account: Option<String>,
        proxy: Option<String>,
        error: Option<String>,
    ) {
        self.api_key_configured = api_key_configured;
        self.api_key_writable = api_key_writable;
        self.api_key_source = api_key_source;
        self.api_key_hint = api_key_hint;
        self.account = account;
        self.proxy = proxy;
        self.error = error;
        self.loading = false;
    }

    /// Current value of the focused field, for prefilling edits: the API
    /// key is never prefilled (the client does not know it); account and
    /// proxy start from their current value.
    fn current_value(&self, field: &str) -> String {
        match field {
            "account" => self.account.clone().unwrap_or_default(),
            "proxy" => self.proxy.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> LoginAction {
        // ---- editing: Enter confirms (sends), Esc cancels ----
        if let Some(edit) = self.editing.take() {
            match edit {
                Edit::Input { buf } => match key.code {
                    KeyCode::Enter => {
                        let field = FIELDS[self.pos].0;
                        return LoginAction::Set { field, value: buf.trim().to_string() };
                    }
                    KeyCode::Esc => return LoginAction::None,
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
            }
            return LoginAction::None;
        }

        // ---- browsing ----
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => LoginAction::Exit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.pos = self.pos.saturating_sub(1);
                LoginAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.pos = (self.pos + 1).min(FIELDS.len() - 1);
                LoginAction::None
            }
            KeyCode::Enter => {
                let field = FIELDS[self.pos].0;
                // An env-supplied key is read-only — refuse to edit it and
                // surface why (the bridge answers the write attempt with a
                // clear message either way, but never start a doomed edit).
                if field == "apiKey" && self.api_key_configured && !self.api_key_writable {
                    return LoginAction::None;
                }
                self.editing = Some(Edit::Input { buf: self.current_value(field) });
                LoginAction::None
            }
            _ => LoginAction::None,
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

    fn type_text(s: &mut LoginState, text: &str) {
        for c in text.chars() {
            s.handle_key(&key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn up_down_move_and_clamp() {
        let mut s = LoginState::default();
        s.handle_key(&key(KeyCode::Down));
        assert_eq!(s.pos, 1);
        s.handle_key(&key(KeyCode::Down));
        s.handle_key(&key(KeyCode::Down));
        assert_eq!(s.pos, 2, "clamped at the last field");
        s.handle_key(&key(KeyCode::Up));
        assert_eq!(s.pos, 1);
        s.handle_key(&key(KeyCode::Up));
        s.handle_key(&key(KeyCode::Up));
        assert_eq!(s.pos, 0, "clamped at the first field");
    }

    #[test]
    fn enter_edits_account_and_sends_the_value() {
        let mut s = LoginState::default();
        s.account = Some("旧账号".into());
        s.pos = 1;
        s.handle_key(&key(KeyCode::Enter));
        // Account edits are prefilled with the current value.
        assert_eq!(s.editing, Some(Edit::Input { buf: "旧账号".into() }));
        // Replace semantics: backspace the prefill, then type the new value.
        for _ in 0.."旧账号".chars().count() {
            s.handle_key(&key(KeyCode::Backspace));
        }
        type_text(&mut s, "新账号");
        let action = s.handle_key(&key(KeyCode::Enter));
        assert_eq!(
            action,
            LoginAction::Set { field: "account", value: "新账号".into() }
        );
        assert!(s.editing.is_none());
    }

    #[test]
    fn api_key_edit_starts_empty_and_esc_cancels() {
        let mut s = LoginState::default();
        s.api_key_configured = true;
        s.api_key_writable = true;
        s.api_key_hint = Some("…1234".into());
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(
            s.editing,
            Some(Edit::Input { buf: String::new() }),
            "the secret is never prefilled"
        );
        type_text(&mut s, "sk-secret");
        s.handle_key(&key(KeyCode::Esc));
        assert!(s.editing.is_none(), "Esc cancels the edit");
        // Env-supplied keys are read-only: Enter refuses to start an edit.
        s.api_key_writable = false;
        s.handle_key(&key(KeyCode::Enter));
        assert!(s.editing.is_none(), "read-only key cannot be edited");
    }

    #[test]
    fn esc_exits_the_panel() {
        let mut s = LoginState::default();
        assert_eq!(s.handle_key(&key(KeyCode::Esc)), LoginAction::Exit);
        assert_eq!(s.handle_key(&key(KeyCode::Char('q'))), LoginAction::Exit);
    }

    #[test]
    fn login_frame_updates_state_and_clears_error() {
        let mut s = LoginState::default();
        s.error = Some("旧错误".into());
        s.apply(
            true,
            true,
            Some("file".into()),
            Some("…abcd".into()),
            Some("a1b2c3d4-0000-0000-0000-000000000000".into()),
            Some("http://127.0.0.1:7890".into()),
            None,
        );
        assert!(s.api_key_configured);
        assert_eq!(s.api_key_hint.as_deref(), Some("…abcd"));
        assert_eq!(s.account.as_deref(), Some("a1b2c3d4-0000-0000-0000-000000000000"));
        assert_eq!(s.proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(s.error, None);
        assert!(!s.loading);
    }
}
