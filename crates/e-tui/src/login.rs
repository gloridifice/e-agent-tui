//! /login panel (D33): the input bar becomes a login page with a two-way
//! menu — API key / Proxy — each opening a sub-page.
//!
//! - API key: lists the model providers; Enter opens that provider's API key
//!   entry (the secret is typed fresh, never prefilled or read back).
//! - Proxy: lists saved proxy routes plus `+ New`; the create form collects
//!   base URL, API key, protocol (three choices) and model name — none of
//!   which is mandatory except a non-empty base URL.

#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    action::AgentRequest,
    agent::{CredentialProvider, ProxyRoute},
    page_core::{handle_text_input, TextEditResult, TextEditor},
};

/// Wire protocols a custom proxy route may speak (first = default).
pub const PROTOCOLS: &[(&str, &str)] = &[
    ("openai-completions", "OpenAI Chat Completions"),
    ("openai-responses", "OpenAI Responses"),
    ("anthropic-messages", "Anthropic Messages"),
];

/// Index of the "save and create" row in the proxy form (after the 4 fields).
pub const PROXY_SAVE_ROW: usize = 4;

/// The sub-page currently shown.
#[derive(Debug, PartialEq)]
pub enum Page {
    /// 二选一: API key / Proxy.
    Menu,
    /// API key: the provider list.
    Providers,
    /// Typing one provider's API key.
    ApiKey { provider: String, buf: String },
    /// Proxy: saved routes + `+ New`.
    ProxyList,
    /// Proxy create form: 4 fields + a save row.
    ProxyForm,
    /// Confirmation before deleting an existing proxy route.
    ProxyDelete { id: String, name: String },
}

/// What the UI should do after a key press.
#[derive(Debug)]
pub enum LoginAction {
    None,
    Exit,
    /// Send one bridge message.
    Send(AgentRequest),
}

/// The four proxy-form fields plus a save row, in display order.
pub const PROXY_ROWS: usize = 5;

/// Draft values of the proxy create form.
#[derive(Debug, Default, PartialEq)]
pub struct ProxyDraft {
    pub base_url: String,
    pub api_key: String,
    pub protocol: usize,
    pub model: String,
}

pub struct LoginState {
    /// Current sub-page.
    pub page: Page,
    /// Selection within the current list page (also the proxy-form row).
    pub pos: usize,
    /// In-progress text edit of a proxy form field (None while browsing).
    pub editing: Option<String>,
    pub error: Option<String>,
    pub loading: bool,
    // ---- bridge-synced state (the `login` frame) ----
    pub providers: Vec<CredentialProvider>,
    pub proxies: Vec<ProxyRoute>,
    /// Proxy create form draft.
    pub draft: ProxyDraft,
}

impl Default for LoginState {
    fn default() -> Self {
        Self {
            page: Page::Menu,
            pos: 0,
            editing: None,
            error: None,
            loading: true,
            providers: Vec::new(),
            proxies: Vec::new(),
            draft: ProxyDraft::default(),
        }
    }
}
/// One bridge `login` frame — the value VIEW only: secrets never cross the wire.
#[derive(Debug, Clone, Default)]
pub struct LoginView {
    pub providers: Vec<CredentialProvider>,
    pub proxies: Vec<ProxyRoute>,
    pub error: Option<String>,
}

impl LoginState {
    /// Apply one bridge `login` frame.
    pub fn apply(&mut self, view: LoginView) {
        let edited_provider = match &self.page {
            Page::ApiKey { provider, .. } => Some(provider.clone()),
            _ => None,
        };
        let deleting_proxy = match &self.page {
            Page::ProxyDelete { id, .. } => Some(id.clone()),
            _ => None,
        };
        let focused_provider = if matches!(self.page, Page::Providers) {
            self.providers
                .get(self.pos)
                .map(|provider| provider.id.clone())
        } else {
            None
        };
        let focused_proxy = if matches!(self.page, Page::ProxyList) {
            self.proxies.get(self.pos).map(|proxy| proxy.id.clone())
        } else {
            None
        };
        self.providers = view.providers;
        self.proxies = view.proxies;
        self.error = view.error;
        self.loading = false;

        if edited_provider.is_some_and(|id| {
            !self
                .providers
                .iter()
                .any(|provider| provider.id == id && provider.api_key_writable)
        }) {
            self.editing = None;
            self.page = Page::Providers;
        }
        if deleting_proxy.is_some_and(|id| !self.proxies.iter().any(|proxy| proxy.id == id)) {
            self.page = Page::ProxyList;
        }

        self.pos = focused_provider
            .and_then(|id| self.providers.iter().position(|provider| provider.id == id))
            .or_else(|| {
                focused_proxy.and_then(|id| self.proxies.iter().position(|proxy| proxy.id == id))
            })
            .unwrap_or_else(|| self.pos.min(self.list_len().saturating_sub(1)));
        self.clamp_to_actionable();
    }

    /// Number of selectable rows on the current list page (menu/providers/
    /// proxy-list). Proxy-list has one extra `+ New` row.
    fn list_len(&self) -> usize {
        match self.page {
            Page::Menu => 2,
            Page::Providers => self.providers.len(),
            Page::ProxyList => self.proxies.len() + 1,
            Page::ProxyDelete { .. } => 2,
            _ => 1,
        }
    }
    fn actionable(&self, index: usize) -> bool {
        match self.page {
            Page::Providers => self
                .providers
                .get(index)
                .is_some_and(|provider| provider.api_key_writable),
            _ => index < self.list_len(),
        }
    }

    fn clamp_to_actionable(&mut self) {
        if self.actionable(self.pos) {
            return;
        }
        if let Some(index) = (0..self.list_len()).find(|index| self.actionable(*index)) {
            self.pos = index;
        } else {
            self.pos = 0;
        }
    }
    fn move_pos(&mut self, delta: i32) {
        if self.list_len() == 0 {
            self.pos = 0;
            return;
        }
        let mut next = self.pos as i32;
        loop {
            let candidate = next + delta;
            if candidate < 0 || candidate >= self.list_len() as i32 {
                break;
            }
            next = candidate;
            if self.actionable(next as usize) {
                self.pos = next as usize;
                break;
            }
        }
    }

    fn open_menu_item(&mut self) {
        match self.pos {
            0 => self.page = Page::Providers,
            _ => self.page = Page::ProxyList,
        }
        self.pos = 0;
    }

    fn proxy_field_value(&self, field: usize) -> String {
        match field {
            0 => self.draft.base_url.clone(),
            1 => self.draft.api_key.clone(),
            2 => PROTOCOLS[self.draft.protocol % PROTOCOLS.len()]
                .0
                .to_string(),
            _ => self.draft.model.clone(),
        }
    }

    /// Start editing the focused proxy-form field (secret fields start empty).
    fn begin_proxy_edit(&mut self) {
        let field = self.pos;
        let buf = if field == 1 {
            String::new()
        } else {
            self.proxy_field_value(field)
        };
        self.editing = Some(buf);
    }

    /// Commit the edited proxy-form field back into the draft.
    fn commit_proxy_edit(&mut self, value: String) {
        let value = value.trim().to_string();
        match self.pos {
            0 => self.draft.base_url = value,
            1 => self.draft.api_key = value,
            3 => self.draft.model = value,
            _ => {}
        }
    }

    pub fn key_scope(&self) -> crate::key_mapping::Scope {
        if self.editing.is_some() {
            crate::key_mapping::Scope::PageEdit
        } else {
            crate::key_mapping::Scope::Page
        }
    }

    #[cfg(test)]
    pub fn handle_key(&mut self, key: &KeyEvent) -> LoginAction {
        self.handle_input(crate::key_mapping::KeyMapping::default().input(self.key_scope(), key))
    }

    pub fn handle_input(&mut self, key: crate::key_mapping::MappedKey) -> LoginAction {
        use crate::key_mapping::{Action, MappedKey::Command};
        if let Some(buf) = self.editing.take() {
            let mut editor = TextEditor {
                buf,
                secret: matches!(self.page, Page::ApiKey { .. })
                    || matches!(self.page, Page::ProxyForm if self.pos == 1),
            };
            return match handle_text_input(&mut editor, key) {
                TextEditResult::Confirm(buf) => {
                    if let Page::ApiKey { provider, .. } = &self.page {
                        let provider = provider.clone();
                        let action = LoginAction::Send(AgentRequest::LoginSetApiKey {
                            provider,
                            value: buf.trim().to_string(),
                        });
                        self.page = Page::Providers;
                        self.pos = 0;
                        self.clamp_to_actionable();
                        action
                    } else {
                        self.commit_proxy_edit(buf);
                        self.pos = (self.pos + 1).min(PROXY_SAVE_ROW);
                        LoginAction::None
                    }
                }
                TextEditResult::Cancel => {
                    if matches!(self.page, Page::ApiKey { .. }) {
                        self.page = Page::Providers;
                        self.pos = 0;
                        self.clamp_to_actionable();
                    }
                    LoginAction::None
                }
                TextEditResult::Continue => {
                    self.editing = Some(editor.buf);
                    LoginAction::None
                }
            };
        }

        // ---- browsing ----
        match (&self.page, key) {
            (Page::Menu, Command(Action::Back | Action::Close)) => LoginAction::Exit,
            (Page::Menu, Command(Action::MoveUp)) => {
                self.pos = self.pos.saturating_sub(1);
                LoginAction::None
            }
            (Page::Menu, Command(Action::MoveDown)) => {
                self.pos = (self.pos + 1).min(1);
                LoginAction::None
            }
            (Page::Menu, Command(Action::Confirm)) => {
                self.open_menu_item();
                LoginAction::None
            }

            (Page::Providers, Command(Action::Back)) => {
                self.page = Page::Menu;
                self.pos = 0;
                LoginAction::None
            }
            (Page::Providers, Command(Action::MoveUp)) => {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::Providers, Command(Action::MoveDown)) => {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::Providers, Command(Action::Confirm)) => {
                if let Some(p) = self
                    .providers
                    .get(self.pos)
                    .filter(|provider| provider.api_key_writable)
                {
                    self.page = Page::ApiKey {
                        provider: p.id.clone(),
                        buf: String::new(),
                    };
                    self.editing = Some(String::new());
                }
                LoginAction::None
            }

            (Page::ApiKey { .. }, _) => LoginAction::None,

            (Page::ProxyList, Command(Action::Back)) => {
                self.page = Page::Menu;
                self.pos = 0;
                LoginAction::None
            }
            (Page::ProxyList, Command(Action::MoveUp)) => {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::ProxyList, Command(Action::MoveDown)) => {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::ProxyList, Command(Action::Confirm)) => {
                if self.pos == self.proxies.len() {
                    self.page = Page::ProxyForm;
                    self.pos = 0;
                    self.draft = ProxyDraft::default();
                } else if let Some(proxy) = self.proxies.get(self.pos) {
                    self.page = Page::ProxyDelete {
                        id: proxy.id.clone(),
                        name: proxy.name.clone(),
                    };
                    self.pos = 0;
                }
                LoginAction::None
            }

            (Page::ProxyForm, Command(Action::Back)) => {
                self.page = Page::ProxyList;
                self.pos = self.proxies.len();
                LoginAction::None
            }
            (Page::ProxyForm, Command(Action::MoveUp)) => {
                self.pos = self.pos.saturating_sub(1);
                LoginAction::None
            }
            (Page::ProxyForm, Command(Action::MoveDown)) => {
                self.pos = (self.pos + 1).min(PROXY_SAVE_ROW);
                LoginAction::None
            }
            (Page::ProxyForm, Command(Action::Confirm)) => {
                if self.pos == PROXY_SAVE_ROW {
                    let d = &self.draft;
                    let action = LoginAction::Send(AgentRequest::LoginProxyCreate {
                        base_url: d.base_url.trim().to_string(),
                        api_key: d.api_key.trim().to_string(),
                        protocol: PROTOCOLS[d.protocol % PROTOCOLS.len()].0.to_string(),
                        model: d.model.trim().to_string(),
                    });
                    self.page = Page::ProxyList;
                    self.pos = self.proxies.len();
                    action
                } else if self.pos == 2 {
                    self.draft.protocol = (self.draft.protocol + 1) % PROTOCOLS.len();
                    LoginAction::None
                } else {
                    self.begin_proxy_edit();
                    LoginAction::None
                }
            }

            (Page::ProxyDelete { id, .. }, Command(Action::Back | Action::Close)) => {
                let proxy_pos = self
                    .proxies
                    .iter()
                    .position(|proxy| proxy.id == *id)
                    .unwrap_or(0);
                self.page = Page::ProxyList;
                self.pos = proxy_pos;
                LoginAction::None
            }
            (Page::ProxyDelete { .. }, Command(Action::MoveLeft | Action::MoveUp)) => {
                self.pos = 0;
                LoginAction::None
            }
            (Page::ProxyDelete { .. }, Command(Action::MoveRight | Action::MoveDown)) => {
                self.pos = 1;
                LoginAction::None
            }
            (Page::ProxyDelete { id, .. }, Command(Action::Confirm)) => {
                let proxy_pos = self
                    .proxies
                    .iter()
                    .position(|proxy| proxy.id == *id)
                    .unwrap_or(0);
                if self.pos == 0 {
                    self.page = Page::ProxyList;
                    self.pos = proxy_pos;
                    LoginAction::None
                } else {
                    let id = id.clone();
                    self.page = Page::ProxyList;
                    self.pos = proxy_pos.min(self.proxies.len().saturating_sub(1));
                    LoginAction::Send(AgentRequest::LoginProxyDelete { id })
                }
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
    fn menu_opens_subpages() {
        let mut s = LoginState::default();
        assert_eq!(s.page, Page::Menu);
        s.handle_key(&key(KeyCode::Enter)); // API key
        assert_eq!(s.page, Page::Providers);
        s.handle_key(&key(KeyCode::Esc));
        s.pos = 1;
        s.handle_key(&key(KeyCode::Enter)); // Proxy
        assert_eq!(s.page, Page::ProxyList);
    }

    #[test]
    fn provider_enter_opens_api_key_edit_and_sends() {
        let mut s = LoginState::default();
        s.providers.push(CredentialProvider {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            api_key_configured: false,
            api_key_writable: true,
            api_key_source: None,
            api_key_hint: None,
        });
        s.page = Page::Providers;
        s.handle_key(&key(KeyCode::Enter));
        assert!(matches!(s.page, Page::ApiKey { .. }));
        assert_eq!(s.editing, Some(String::new()), "the secret starts empty");
        type_text(&mut s, "sk-secret");
        let action = s.handle_key(&key(KeyCode::Enter));
        assert!(matches!(
            action,
            LoginAction::Send(AgentRequest::LoginSetApiKey { provider, value })
                if provider == "deepseek" && value == "sk-secret"
        ));
        assert_eq!(s.page, Page::Providers);
    }

    #[test]
    fn proxy_form_cycles_protocol_and_saves() {
        let mut s = LoginState::default();
        s.page = Page::ProxyForm;
        // base URL (pos 0): Enter begins editing, Enter commits + advances.
        s.handle_key(&key(KeyCode::Enter));
        type_text(&mut s, "https://example.com/v1");
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(s.draft.base_url, "https://example.com/v1");
        // apiKey (pos 1): secret starts empty even though the field has a value.
        s.handle_key(&key(KeyCode::Enter));
        type_text(&mut s, "sk-key");
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(s.draft.api_key, "sk-key");
        // protocol (pos 2): Enter cycles the choice, no advance.
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(s.draft.protocol, 1);
        // model (pos 3).
        s.handle_key(&key(KeyCode::Down));
        s.handle_key(&key(KeyCode::Enter));
        type_text(&mut s, "gpt-4o");
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(s.draft.model, "gpt-4o");
        // save row
        let action = s.handle_key(&key(KeyCode::Enter));
        assert!(matches!(
            action,
            LoginAction::Send(AgentRequest::LoginProxyCreate { protocol, .. })
                if protocol == "openai-responses"
        ));
        assert_eq!(s.page, Page::ProxyList);
    }

    #[test]
    fn non_writable_provider_is_not_actionable() {
        let mut s = LoginState::default();
        s.providers.push(CredentialProvider {
            id: "env-only".into(),
            name: "Env only".into(),
            api_key_configured: true,
            api_key_writable: false,
            api_key_source: Some("env".into()),
            api_key_hint: Some("…1234".into()),
        });
        s.page = Page::Providers;
        s.handle_key(&key(KeyCode::Enter));
        assert_eq!(s.page, Page::Providers);
        assert!(s.editing.is_none());
    }

    #[test]
    fn proxy_delete_requires_explicit_confirmation() {
        let mut s = LoginState::default();
        s.proxies.push(ProxyRoute {
            id: "proxy-1".into(),
            name: "Local".into(),
            base_url: "http://localhost".into(),
            protocol: "openai-responses".into(),
            model: String::new(),
        });
        s.page = Page::ProxyList;
        s.handle_key(&key(KeyCode::Enter));
        assert!(matches!(s.page, Page::ProxyDelete { .. }));
        assert!(matches!(
            s.handle_key(&key(KeyCode::Enter)),
            LoginAction::None
        ));

        s.page = Page::ProxyList;
        s.pos = 0;
        s.handle_key(&key(KeyCode::Enter));
        s.handle_key(&key(KeyCode::Right));
        assert!(matches!(
            s.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::LoginProxyDelete { id }) if id == "proxy-1"
        ));
    }

    #[test]
    fn provider_refresh_reconciles_focus_by_stable_id() {
        let provider = |id: &str| CredentialProvider {
            id: id.into(),
            name: id.into(),
            api_key_configured: false,
            api_key_writable: true,
            api_key_source: None,
            api_key_hint: None,
        };
        let mut s = LoginState::default();
        s.page = Page::Providers;
        s.providers = vec![provider("a"), provider("b")];
        s.pos = 1;
        s.apply(LoginView {
            providers: vec![provider("b"), provider("a")],
            proxies: Vec::new(),
            error: None,
        });
        assert_eq!(s.providers[s.pos].id, "b");
    }

    #[test]
    fn api_key_editor_treats_hjkl_as_secret_text() {
        let mut s = LoginState::default();
        s.page = Page::ApiKey {
            provider: "p".into(),
            buf: String::new(),
        };
        s.editing = Some(String::new());
        for ch in ['h', 'j', 'k', 'l'] {
            s.handle_key(&key(KeyCode::Char(ch)));
        }
        assert_eq!(s.editing.as_deref(), Some("hjkl"));
    }

    #[test]
    fn refresh_closes_actions_whose_stable_target_disappeared() {
        let mut key_page = LoginState {
            page: Page::ApiKey {
                provider: "gone".into(),
                buf: String::new(),
            },
            editing: Some("secret".into()),
            ..Default::default()
        };
        key_page.apply(LoginView::default());
        assert_eq!(key_page.page, Page::Providers);
        assert!(key_page.editing.is_none());

        let mut delete_page = LoginState {
            page: Page::ProxyDelete {
                id: "gone".into(),
                name: "Gone".into(),
            },
            ..Default::default()
        };
        delete_page.apply(LoginView::default());
        assert_eq!(delete_page.page, Page::ProxyList);
    }
}
