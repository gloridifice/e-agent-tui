//! Provider-neutral login page state. DSH uses API-key and proxy subpages;
//! adapters with native authentication capabilities use provider/method,
//! interactive prompt, guidance, and credential-removal subpages.

#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    action::AgentRequest,
    agent::{
        AuthNotice, AuthOutcomeKind, AuthPrompt, AuthPromptKind, AuthProvider, CredentialProvider,
        ProxyRoute,
    },
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
    /// Native authentication provider selection.
    NativeProviders,
    /// Native method selection for one provider.
    NativeMethods { provider: String },
    /// Confirmation before removing a native stored credential.
    NativeLogout { provider: String },
    /// A provider-owned text, secret, or choice prompt.
    NativePrompt(AuthPrompt),
    /// Native code is waiting on the provider or runtime synchronization.
    NativeWaiting { flow_id: Option<String> },
    /// Terminal native authentication result.
    NativeOutcome {
        outcome: AuthOutcomeKind,
        message: String,
    },
}

/// What the UI should do after a key press.
#[derive(Debug)]
pub enum LoginAction {
    None,
    Exit,
    /// Send one bridge message.
    Send(AgentRequest),
    Copy(String),
    /// Cancel the active native flow and close the page.
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeNoticeAction {
    OpenUrl(String),
    CopyUrl(String),
    CopyCode(String),
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
    pub auth_providers: Vec<AuthProvider>,
    pub auth_notices: Vec<AuthNotice>,
    pub auth_logout: bool,
    pub provider_ref: Option<String>,
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
            auth_providers: Vec::new(),
            auth_notices: Vec::new(),
            auth_logout: false,
            provider_ref: None,
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
    pub fn native_loading(provider_ref: Option<String>, logout: bool) -> Self {
        Self {
            page: Page::NativeProviders,
            provider_ref,
            auth_logout: logout,
            ..Self::default()
        }
    }

    pub fn apply_auth_catalog(
        &mut self,
        providers: Vec<AuthProvider>,
        provider_ref: Option<String>,
        logout: bool,
        error: Option<String>,
    ) {
        self.auth_providers = if logout {
            providers
                .into_iter()
                .filter(|provider| Self::provider_actionable(provider, true))
                .collect()
        } else {
            providers
        };
        self.provider_ref = provider_ref.or_else(|| self.provider_ref.take());
        self.auth_logout = logout;
        self.error = error;
        self.loading = false;
        self.page = Page::NativeProviders;
        self.pos = 0;
        if let Some(reference) = self.provider_ref.as_deref() {
            if let Some(exact) = self
                .auth_providers
                .iter()
                .position(|provider| provider.id.eq_ignore_ascii_case(reference))
            {
                self.pos = exact;
            } else {
                let matches = self
                    .auth_providers
                    .iter()
                    .enumerate()
                    .filter(|(_, provider)| provider.name.eq_ignore_ascii_case(reference))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                match matches.as_slice() {
                    [index] => self.pos = *index,
                    [] => {
                        self.error = Some(format!("Unknown authentication provider: {reference}"))
                    }
                    _ => {
                        self.error = Some(format!("Ambiguous authentication provider: {reference}"))
                    }
                }
            }
        }
    }

    pub fn start_auth(&mut self, flow_id: String) {
        self.editing = None;
        self.auth_notices.clear();
        self.loading = false;
        self.page = Page::NativeWaiting {
            flow_id: Some(flow_id),
        };
    }

    pub fn apply_auth_prompt(&mut self, prompt: AuthPrompt) {
        if !self.accepts_flow(&prompt.flow_id) {
            return;
        }
        self.editing = match prompt.kind {
            AuthPromptKind::Text | AuthPromptKind::Secret => Some(String::new()),
            AuthPromptKind::ManualCode | AuthPromptKind::Select => None,
        };
        self.pos = 0;
        self.loading = false;
        self.page = Page::NativePrompt(prompt);
    }

    pub fn withdraw_auth_prompt(&mut self, flow_id: &str, prompt_id: &str) {
        if matches!(&self.page, Page::NativePrompt(prompt)
            if prompt.flow_id == flow_id && prompt.prompt_id == prompt_id)
        {
            self.editing = None;
            self.page = Page::NativeWaiting {
                flow_id: Some(flow_id.to_owned()),
            };
        }
    }

    pub fn apply_auth_notice(&mut self, notice: AuthNotice) {
        if !self.accepts_flow(&notice.flow_id) {
            return;
        }
        let flow_id = notice.flow_id.clone();
        self.auth_notices.push(notice);
        if self.auth_notices.len() > 8 {
            self.auth_notices.remove(0);
        }
        if matches!(self.page, Page::NativeWaiting { .. }) {
            self.page = Page::NativeWaiting {
                flow_id: Some(flow_id),
            };
            self.pos = self.pos.min(self.row_count().saturating_sub(1));
        }
    }

    pub fn finish_auth(&mut self, flow_id: &str, outcome: AuthOutcomeKind, message: String) {
        if !self.accepts_flow(flow_id) {
            return;
        }
        self.editing = None;
        self.loading = false;
        self.page = Page::NativeOutcome { outcome, message };
    }

    fn accepts_flow(&self, flow_id: &str) -> bool {
        match &self.page {
            Page::NativeWaiting {
                flow_id: Some(active),
            } => active == flow_id,
            Page::NativePrompt(prompt) => prompt.flow_id == flow_id,
            _ => false,
        }
    }

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
            .unwrap_or_else(|| self.pos.min(self.row_count().saturating_sub(1)));
        self.clamp_to_actionable();
    }

    pub fn native_notice_actions(&self) -> Vec<NativeNoticeAction> {
        let mut actions = Vec::new();
        for notice in &self.auth_notices {
            if let Some(url) = notice.url.as_ref() {
                actions.push(NativeNoticeAction::OpenUrl(url.clone()));
                actions.push(NativeNoticeAction::CopyUrl(url.clone()));
            }
            if let Some(code) = notice.code.as_ref() {
                actions.push(NativeNoticeAction::CopyCode(code.clone()));
            }
        }
        actions
    }

    /// Whether a native provider row may be selected. The single eligibility
    /// rule shared by cursor movement, focus building, and rendering.
    pub fn provider_actionable(provider: &AuthProvider, logout: bool) -> bool {
        if logout {
            provider.removable
        } else {
            !provider.methods.is_empty()
        }
    }

    /// Stable focus identity of a cursor row on a native page.
    pub fn row_focus_id(&self, index: usize) -> Option<String> {
        if !self.row_actionable(index) {
            return None;
        }
        match &self.page {
            Page::NativeProviders => self
                .auth_providers
                .get(index)
                .map(|provider| format!("auth:provider:{}", provider.id)),
            Page::NativeMethods { provider } => self
                .auth_providers
                .iter()
                .find(|candidate| candidate.id == *provider)?
                .methods
                .get(index)
                .map(|method| format!("auth:method:{}", method.id)),
            Page::NativeLogout { provider } => match index {
                0 => Some(format!("auth:logout:{provider}:cancel")),
                1 => Some(format!("auth:logout:{provider}:remove")),
                _ => None,
            },
            Page::NativePrompt(prompt) if prompt.kind == AuthPromptKind::Select => prompt
                .options
                .get(index)
                .map(|option| format!("auth:prompt:{}:{}", prompt.prompt_id, option.value)),
            _ => None,
        }
    }

    /// Number of selectable rows on the current list page (menu/providers/
    /// proxy-list). Proxy-list has one extra `+ New` row.
    pub fn row_count(&self) -> usize {
        match self.page {
            Page::Menu => 2,
            Page::Providers => self.providers.len(),
            Page::ProxyList => self.proxies.len() + 1,
            Page::ProxyDelete { .. } => 2,
            Page::NativeProviders => self.auth_providers.len(),
            Page::NativeMethods { ref provider } => self
                .auth_providers
                .iter()
                .find(|candidate| candidate.id == *provider)
                .map_or(0, |candidate| candidate.methods.len()),
            Page::NativeLogout { .. } => 2,
            Page::NativeWaiting { .. } => self.native_notice_actions().len().max(1),
            Page::NativePrompt(ref prompt) if prompt.kind == AuthPromptKind::Select => {
                prompt.options.len()
            }
            Page::NativePrompt(ref prompt) if prompt.kind == AuthPromptKind::ManualCode => {
                self.native_notice_actions().len() + 1
            }
            _ => 1,
        }
    }
    pub fn row_actionable(&self, index: usize) -> bool {
        match self.page {
            Page::Providers => self
                .providers
                .get(index)
                .is_some_and(|provider| provider.api_key_writable),
            Page::NativeProviders => self
                .auth_providers
                .get(index)
                .is_some_and(|provider| Self::provider_actionable(provider, self.auth_logout)),
            _ => index < self.row_count(),
        }
    }

    fn clamp_to_actionable(&mut self) {
        if self.row_actionable(self.pos) {
            return;
        }
        if let Some(index) = (0..self.row_count()).find(|index| self.row_actionable(*index)) {
            self.pos = index;
        } else {
            self.pos = 0;
        }
    }
    fn move_pos(&mut self, delta: i32) {
        if self.row_count() == 0 {
            self.pos = 0;
            return;
        }
        let mut next = self.pos as i32;
        loop {
            let candidate = next + delta;
            if candidate < 0 || candidate >= self.row_count() as i32 {
                break;
            }
            next = candidate;
            if self.row_actionable(next as usize) {
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
                    || matches!(self.page, Page::ProxyForm if self.pos == 1)
                    || matches!(&self.page, Page::NativePrompt(prompt)
                        if matches!(prompt.kind, AuthPromptKind::Secret | AuthPromptKind::ManualCode)),
            };
            return match handle_text_input(&mut editor, key) {
                TextEditResult::Confirm(buf) => {
                    if let Page::NativePrompt(prompt) = &self.page {
                        let (flow_id, prompt_id) =
                            (prompt.flow_id.clone(), prompt.prompt_id.clone());
                        self.page = Page::NativeWaiting {
                            flow_id: Some(flow_id.clone()),
                        };
                        LoginAction::Send(AgentRequest::AuthReply {
                            flow_id,
                            prompt_id,
                            value: buf,
                        })
                    } else if let Page::ApiKey { provider, .. } = &self.page {
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
                    if let Page::NativePrompt(prompt) = &self.page {
                        self.page = Page::NativeWaiting {
                            flow_id: Some(prompt.flow_id.clone()),
                        };
                        LoginAction::Send(AgentRequest::AuthCancel)
                    } else {
                        if matches!(self.page, Page::ApiKey { .. }) {
                            self.page = Page::Providers;
                            self.pos = 0;
                            self.clamp_to_actionable();
                        }
                        LoginAction::None
                    }
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

            (Page::NativeProviders, Command(Action::Back | Action::Close)) => LoginAction::Exit,
            (Page::NativeProviders, Command(Action::MoveUp)) => {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::NativeProviders, Command(Action::MoveDown)) => {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::NativeProviders, Command(Action::Confirm)) => {
                let Some(provider) = self.auth_providers.get(self.pos) else {
                    return LoginAction::None;
                };
                if self.auth_logout {
                    self.page = Page::NativeLogout {
                        provider: provider.id.clone(),
                    };
                    self.pos = 0;
                    return LoginAction::None;
                }
                if provider.methods.len() == 1 {
                    let request = AgentRequest::AuthStart {
                        provider: provider.id.clone(),
                        method: Some(provider.methods[0].id.clone()),
                        logout: false,
                    };
                    self.auth_notices.clear();
                    self.page = Page::NativeWaiting { flow_id: None };
                    return LoginAction::Send(request);
                }
                self.page = Page::NativeMethods {
                    provider: provider.id.clone(),
                };
                self.pos = 0;
                LoginAction::None
            }
            (Page::NativeMethods { .. }, Command(Action::Back)) => {
                self.page = Page::NativeProviders;
                self.pos = 0;
                LoginAction::None
            }
            (Page::NativeMethods { .. }, Command(Action::MoveUp)) => {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::NativeMethods { .. }, Command(Action::MoveDown)) => {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::NativeMethods { provider }, Command(Action::Confirm)) => {
                let method = self
                    .auth_providers
                    .iter()
                    .find(|candidate| candidate.id == *provider)
                    .and_then(|candidate| candidate.methods.get(self.pos));
                let Some(method) = method else {
                    return LoginAction::None;
                };
                let request = AgentRequest::AuthStart {
                    provider: provider.clone(),
                    method: Some(method.id.clone()),
                    logout: false,
                };
                self.auth_notices.clear();
                self.page = Page::NativeWaiting { flow_id: None };
                LoginAction::Send(request)
            }
            (Page::NativeLogout { .. }, Command(Action::Back)) => {
                self.page = Page::NativeProviders;
                self.pos = 0;
                LoginAction::None
            }
            (Page::NativeLogout { .. }, Command(Action::MoveUp)) => {
                self.pos = self.pos.saturating_sub(1);
                LoginAction::None
            }
            (Page::NativeLogout { .. }, Command(Action::MoveDown)) => {
                self.pos = (self.pos + 1).min(1);
                LoginAction::None
            }
            (Page::NativeLogout { provider }, Command(Action::Confirm)) => {
                if self.pos == 0 {
                    self.page = Page::NativeProviders;
                    self.pos = 0;
                    LoginAction::None
                } else {
                    let request = AgentRequest::AuthStart {
                        provider: provider.clone(),
                        method: None,
                        logout: true,
                    };
                    self.page = Page::NativeWaiting { flow_id: None };
                    LoginAction::Send(request)
                }
            }
            (Page::NativeWaiting { .. }, Command(Action::MoveUp)) => {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::NativeWaiting { .. }, Command(Action::MoveDown)) => {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::NativeWaiting { flow_id }, Command(Action::Confirm)) => {
                let Some(flow_id) = flow_id.clone() else {
                    return LoginAction::None;
                };
                match self.native_notice_actions().get(self.pos).cloned() {
                    Some(NativeNoticeAction::OpenUrl(url)) => {
                        LoginAction::Send(AgentRequest::AuthOpenUrl { flow_id, url })
                    }
                    Some(NativeNoticeAction::CopyUrl(value))
                    | Some(NativeNoticeAction::CopyCode(value)) => LoginAction::Copy(value),
                    None => LoginAction::None,
                }
            }
            (Page::NativePrompt(prompt), Command(Action::MoveUp))
                if prompt.kind == AuthPromptKind::ManualCode =>
            {
                self.move_pos(-1);
                LoginAction::None
            }
            (Page::NativePrompt(prompt), Command(Action::MoveDown))
                if prompt.kind == AuthPromptKind::ManualCode =>
            {
                self.move_pos(1);
                LoginAction::None
            }
            (Page::NativePrompt(prompt), Command(Action::Confirm))
                if prompt.kind == AuthPromptKind::ManualCode =>
            {
                let actions = self.native_notice_actions();
                if self.pos == actions.len() {
                    self.editing = Some(String::new());
                    LoginAction::None
                } else {
                    match actions.get(self.pos).cloned() {
                        Some(NativeNoticeAction::OpenUrl(url)) => {
                            LoginAction::Send(AgentRequest::AuthOpenUrl {
                                flow_id: prompt.flow_id.clone(),
                                url,
                            })
                        }
                        Some(NativeNoticeAction::CopyUrl(value))
                        | Some(NativeNoticeAction::CopyCode(value)) => LoginAction::Copy(value),
                        None => LoginAction::None,
                    }
                }
            }
            (Page::NativePrompt(prompt), Command(Action::MoveUp))
                if prompt.kind == AuthPromptKind::Select =>
            {
                self.pos = self.pos.saturating_sub(1);
                LoginAction::None
            }
            (Page::NativePrompt(prompt), Command(Action::MoveDown))
                if prompt.kind == AuthPromptKind::Select =>
            {
                self.pos = (self.pos + 1).min(prompt.options.len().saturating_sub(1));
                LoginAction::None
            }
            (Page::NativePrompt(prompt), Command(Action::Confirm))
                if prompt.kind == AuthPromptKind::Select =>
            {
                let Some(option) = prompt.options.get(self.pos) else {
                    return LoginAction::None;
                };
                let flow_id = prompt.flow_id.clone();
                let request = AgentRequest::AuthReply {
                    flow_id: flow_id.clone(),
                    prompt_id: prompt.prompt_id.clone(),
                    value: option.value.clone(),
                };
                self.page = Page::NativeWaiting {
                    flow_id: Some(flow_id),
                };
                LoginAction::Send(request)
            }
            (Page::NativePrompt(prompt), Command(Action::Back | Action::Close)) => {
                let flow_id = prompt.flow_id.clone();
                self.editing = None;
                self.page = Page::NativeWaiting {
                    flow_id: Some(flow_id),
                };
                LoginAction::Send(AgentRequest::AuthCancel)
            }
            (Page::NativeWaiting { .. }, Command(Action::Back | Action::Close)) => {
                self.editing = None;
                LoginAction::Cancel
            }
            (
                Page::NativeOutcome { .. },
                Command(Action::Back | Action::Close | Action::Confirm),
            ) => LoginAction::Exit,

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

    fn auth_provider(id: &str, methods: &[&str], removable: bool) -> AuthProvider {
        AuthProvider {
            id: id.into(),
            name: format!("Provider {id}"),
            methods: methods
                .iter()
                .map(|method| crate::agent::AuthMethod {
                    id: (*method).into(),
                    name: (*method).into(),
                    description: None,
                })
                .collect(),
            configured: removable,
            removable,
            source: removable.then(|| "API key".into()),
        }
    }

    #[test]
    fn native_auth_uses_stable_methods_and_ephemeral_secret_replies() {
        let mut state = LoginState::native_loading(Some("two".into()), false);
        state.apply_auth_catalog(
            vec![
                auth_provider("one", &["api_key"], false),
                auth_provider("two", &["api_key", "oauth"], false),
            ],
            None,
            false,
            None,
        );
        assert_eq!(state.pos, 1);
        state.handle_key(&key(KeyCode::Enter));
        assert!(matches!(
            state.page,
            Page::NativeMethods { ref provider } if provider == "two"
        ));
        state.handle_key(&key(KeyCode::Down));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::AuthStart { ref provider, method: Some(ref method), .. })
                if provider == "two" && method == "oauth"
        ));

        state.start_auth("flow".into());
        state.apply_auth_prompt(AuthPrompt {
            flow_id: "flow".into(),
            prompt_id: "secret".into(),
            kind: AuthPromptKind::Secret,
            message: "Secret".into(),
            placeholder: None,
            options: Vec::new(),
        });
        type_text(&mut state, "not-retained");
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::AuthReply { ref flow_id, ref prompt_id, ref value })
                if flow_id == "flow" && prompt_id == "secret" && value == "not-retained"
        ));
        assert!(state.editing.is_none());
        assert!(matches!(state.page, Page::NativeWaiting { .. }));
    }

    #[test]
    fn native_logout_filters_non_stored_credentials_and_rejects_stale_events() {
        let mut state = LoginState::native_loading(None, true);
        state.apply_auth_catalog(
            vec![
                auth_provider("ambient", &["api_key"], false),
                auth_provider("stored", &["api_key"], true),
            ],
            None,
            true,
            None,
        );
        assert_eq!(state.auth_providers.len(), 1);
        assert_eq!(state.auth_providers[0].id, "stored");
        state.start_auth("current".into());
        state.finish_auth("stale", AuthOutcomeKind::Succeeded, "wrong".into());
        assert!(matches!(state.page, Page::NativeWaiting { .. }));
        state.finish_auth("current", AuthOutcomeKind::Succeeded, "done".into());
        assert!(matches!(
            state.page,
            Page::NativeOutcome {
                outcome: AuthOutcomeKind::Succeeded,
                ..
            }
        ));
    }

    #[test]
    fn native_notice_actions_open_and_copy_explicit_values() {
        let mut state = LoginState::native_loading(None, false);
        state.start_auth("flow-actions".into());
        state.apply_auth_notice(AuthNotice {
            flow_id: "flow-actions".into(),
            kind: crate::agent::AuthNoticeKind::DeviceCode,
            message: "Continue in browser".into(),
            url: Some("https://example.test/device".into()),
            code: Some("ABCD-EFGH".into()),
        });
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::AuthOpenUrl { flow_id, url })
                if flow_id == "flow-actions" && url == "https://example.test/device"
        ));
        state.handle_key(&key(KeyCode::Down));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Copy(value) if value == "https://example.test/device"
        ));
        state.handle_key(&key(KeyCode::Down));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Copy(value) if value == "ABCD-EFGH"
        ));
    }

    #[test]
    fn manual_callback_prompt_keeps_notice_actions_before_masked_entry() {
        let mut state = LoginState::native_loading(None, false);
        state.start_auth("flow-manual".into());
        state.apply_auth_notice(AuthNotice {
            flow_id: "flow-manual".into(),
            kind: crate::agent::AuthNoticeKind::AuthorizationUrl,
            message: "Open the authorization page".into(),
            url: Some("https://example.test/auth".into()),
            code: None,
        });
        state.apply_auth_prompt(AuthPrompt {
            flow_id: "flow-manual".into(),
            prompt_id: "manual".into(),
            kind: AuthPromptKind::ManualCode,
            message: "Paste callback URL or code".into(),
            placeholder: None,
            options: Vec::new(),
        });
        assert!(state.editing.is_none());
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::AuthOpenUrl { .. })
        ));
        state.handle_key(&key(KeyCode::Down));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Copy(value) if value == "https://example.test/auth"
        ));
        state.handle_key(&key(KeyCode::Down));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::None
        ));
        assert_eq!(state.editing.as_deref(), Some(""));
    }

    #[test]
    fn native_prompt_preserves_empty_text_answers() {
        let mut state = LoginState::native_loading(None, false);
        state.start_auth("flow-empty".into());
        state.apply_auth_prompt(AuthPrompt {
            flow_id: "flow-empty".into(),
            prompt_id: "text".into(),
            kind: AuthPromptKind::Text,
            message: "Optional account".into(),
            placeholder: None,
            options: Vec::new(),
        });
        assert!(matches!(
            state.handle_key(&key(KeyCode::Enter)),
            LoginAction::Send(AgentRequest::AuthReply { value, .. }) if value.is_empty()
        ));
    }

    #[test]
    fn native_pages_always_offer_a_way_to_cancel_and_leave() {
        let mut state = LoginState::native_loading(None, false);
        state.start_auth("flow".into());
        state.apply_auth_prompt(AuthPrompt {
            flow_id: "flow".into(),
            prompt_id: "select".into(),
            kind: AuthPromptKind::Select,
            message: "Choose an account".into(),
            placeholder: None,
            options: vec![crate::agent::AuthPromptOption {
                value: "one".into(),
                label: "One".into(),
                description: None,
            }],
        });
        assert!(matches!(
            state.handle_key(&key(KeyCode::Esc)),
            LoginAction::Send(AgentRequest::AuthCancel)
        ));
        assert!(matches!(state.page, Page::NativeWaiting { .. }));
        assert!(matches!(
            state.handle_key(&key(KeyCode::Esc)),
            LoginAction::Cancel
        ));
    }

    #[test]
    fn native_prompt_cancellation_drops_the_secret_buffer() {
        let mut state = LoginState::native_loading(None, false);
        state.start_auth("flow".into());
        state.apply_auth_prompt(AuthPrompt {
            flow_id: "flow".into(),
            prompt_id: "secret".into(),
            kind: AuthPromptKind::Secret,
            message: "Secret".into(),
            placeholder: None,
            options: Vec::new(),
        });
        type_text(&mut state, "discard-me");
        assert!(matches!(
            state.handle_key(&key(KeyCode::Esc)),
            LoginAction::Send(AgentRequest::AuthCancel)
        ));
        assert!(state.editing.is_none());
        state.finish_auth(
            "flow",
            AuthOutcomeKind::Cancelled,
            "Authentication cancelled".into(),
        );
        assert!(matches!(
            state.page,
            Page::NativeOutcome {
                outcome: AuthOutcomeKind::Cancelled,
                ..
            }
        ));
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
