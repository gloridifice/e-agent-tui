//! Unified configuration pages that replace the ordinary input bar.
//!
//! The closed page roster keeps domain behavior typed while this module owns
//! the common lifecycle, focus navigation, text editing, viewport anchoring,
//! and side-effect boundary used by the main loop.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub use crate::page_core::{
    direction_from_key, handle_text_editor, Direction, FocusId, FocusNode, FocusState, PageEffect,
    PageOutcome, TextEditResult, TextEditor, ViewportState,
};
use crate::{
    config::Config,
    login::{LoginAction, LoginState, LoginView, Page as LoginPage, PROXY_SAVE_ROW},
    protocol::{ClientMessage, ModelProviderInfo, SessionInfo},
    settings::{items_in, ItemKind, SettingsAction, SettingsState, CATEGORIES},
    theme::ThemeFile,
};

#[derive(Clone)]
pub struct ThemeOption {
    pub name: String,
    pub palette: crate::config::Theme,
}

pub struct ThemePage {
    pub themes: Vec<ThemeOption>,
    pub current: String,
}

/// `/resume` session list. The page is visible immediately in a loading
/// state; the bridge may then send a fast header-only list followed by the
/// same rows enriched with titles.
pub struct ResumePage {
    pub sessions: Vec<SessionInfo>,
    pub query: String,
    /// Selection index within [`Self::filtered_indices`].
    pub sel: usize,
    pub loading: bool,
    pub titles_pending: bool,
}

impl ResumePage {
    pub fn loading() -> Self {
        Self {
            sessions: Vec::new(),
            query: String::new(),
            sel: 0,
            loading: true,
            titles_pending: false,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                query.is_empty()
                    || session.title.to_lowercase().contains(&query)
                    || session.id.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn selected_id(&self) -> Option<&str> {
        let filtered = self.filtered_indices();
        filtered
            .get(self.sel)
            .and_then(|index| self.sessions.get(*index))
            .map(|session| session.id.as_str())
    }

    pub fn apply_sessions(&mut self, sessions: Vec<SessionInfo>, titles_pending: bool) {
        let selected_id = self.selected_id().map(str::to_owned);
        self.sessions = sessions;
        self.loading = false;
        self.titles_pending = titles_pending;
        let filtered = self.filtered_indices();
        self.sel = selected_id
            .as_ref()
            .and_then(|id| {
                filtered.iter().position(|index| {
                    self.sessions
                        .get(*index)
                        .is_some_and(|session| session.id == *id)
                })
            })
            .unwrap_or_else(|| self.sel.min(filtered.len().saturating_sub(1)));
    }

    fn handle_key(&mut self, key: &KeyEvent) -> PageOutcome {
        match key.code {
            KeyCode::Esc => PageOutcome::close(),
            KeyCode::Up => {
                self.sel = self.sel.saturating_sub(1);
                PageOutcome::default()
            }
            KeyCode::Down => {
                self.sel = (self.sel + 1).min(self.filtered_indices().len().saturating_sub(1));
                PageOutcome::default()
            }
            KeyCode::Enter => self
                .selected_id()
                .map(|session_id| {
                    PageOutcome::send(
                        ClientMessage::Attach {
                            session_id: session_id.to_owned(),
                        },
                        true,
                    )
                })
                .unwrap_or_default(),
            KeyCode::Backspace => {
                self.query.pop();
                self.sel = 0;
                PageOutcome::default()
            }
            KeyCode::Char(character)
                if !character.is_ascii_control()
                    && !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
            {
                self.query.push(character);
                self.sel = 0;
                PageOutcome::default()
            }
            _ => PageOutcome::default(),
        }
    }
}

impl ThemePage {
    pub fn from_files(files: &[ThemeFile], current: &str) -> Self {
        Self {
            themes: files
                .iter()
                .map(|file| ThemeOption {
                    name: file.name.clone(),
                    palette: file.to_theme().unwrap_or_default(),
                })
                .collect(),
            current: current.to_owned(),
        }
    }
}

pub struct ModelPage {
    pub providers: Vec<ModelProviderInfo>,
    pub active_provider: Option<String>,
    pub current: Option<(String, String)>,
    pub loading: bool,
}

impl ModelPage {
    pub fn loading() -> Self {
        Self {
            providers: Vec::new(),
            active_provider: None,
            current: None,
            loading: true,
        }
    }

    pub fn apply_catalog(
        &mut self,
        providers: Vec<ModelProviderInfo>,
        current: Option<(String, String)>,
        focus: &mut FocusState,
    ) {
        let had_focus = focus.current.is_some();
        let previous_provider = self.active_provider.clone();
        self.providers = providers;
        self.current = current;
        self.loading = false;
        self.active_provider = previous_provider
            .filter(|id| self.providers.iter().any(|provider| provider.id == *id))
            .or_else(|| {
                self.current
                    .as_ref()
                    .map(|(provider, _)| provider.clone())
                    .filter(|id| self.providers.iter().any(|provider| provider.id == *id))
            })
            .or_else(|| self.providers.first().map(|provider| provider.id.clone()));
        self.rebuild_focus(focus);
        if !had_focus {
            if let Some((provider, model)) = self.current.as_ref() {
                focus.set(Self::model_focus(provider, model));
            }
        }
    }

    pub fn active_index(&self) -> Option<usize> {
        let active = self.active_provider.as_ref()?;
        self.providers
            .iter()
            .position(|provider| provider.id == *active)
    }

    pub fn active_models(&self) -> &[crate::protocol::ModelInfo] {
        self.active_index()
            .and_then(|index| self.providers.get(index))
            .map(|provider| provider.models.as_slice())
            .unwrap_or(&[])
    }

    fn provider_focus(id: &str) -> FocusId {
        FocusId::new(format!("provider:{id}"))
    }

    fn model_focus(provider: &str, model: &str) -> FocusId {
        FocusId::new(format!("model:{provider}:{model}"))
    }

    pub fn rebuild_focus(&self, focus: &mut FocusState) {
        let mut nodes = Vec::new();
        for (index, provider) in self.providers.iter().enumerate() {
            let mut node = FocusNode::new(Self::provider_focus(&provider.id));
            node.up = index
                .checked_sub(1)
                .and_then(|i| self.providers.get(i))
                .map(|item| Self::provider_focus(&item.id));
            node.down = self
                .providers
                .get(index + 1)
                .map(|item| Self::provider_focus(&item.id));
            if self.active_provider.as_deref() == Some(provider.id.as_str()) {
                node.right = provider
                    .models
                    .first()
                    .map(|model| Self::model_focus(&provider.id, &model.id));
            }
            nodes.push(node);
        }
        if let Some(provider_index) = self.active_index() {
            let provider = &self.providers[provider_index];
            for (index, model) in provider.models.iter().enumerate() {
                let mut node = FocusNode::new(Self::model_focus(&provider.id, &model.id));
                node.left = Some(Self::provider_focus(&provider.id));
                node.up = index
                    .checked_sub(1)
                    .and_then(|i| provider.models.get(i))
                    .map(|item| Self::model_focus(&provider.id, &item.id));
                node.down = provider
                    .models
                    .get(index + 1)
                    .map(|item| Self::model_focus(&provider.id, &item.id));
                nodes.push(node);
            }
        }
        focus.replace(nodes);
    }

    fn activate(&mut self, focus: &mut FocusState) -> PageOutcome {
        let Some(id) = focus.current.as_ref().map(|id| id.0.clone()) else {
            return PageOutcome::default();
        };
        if let Some(provider_id) = id.strip_prefix("provider:") {
            self.active_provider = Some(provider_id.to_owned());
            self.rebuild_focus(focus);
            let target = self
                .providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .and_then(|provider| {
                    self.current
                        .as_ref()
                        .filter(|(current_provider, _)| current_provider == provider_id)
                        .and_then(|(_, current_model)| {
                            provider
                                .models
                                .iter()
                                .find(|model| model.id == *current_model)
                        })
                        .or_else(|| provider.models.first())
                        .map(|model| Self::model_focus(provider_id, &model.id))
                });
            if let Some(target) = target {
                focus.set(target);
            }
            return PageOutcome::default();
        }
        let selection = self.providers.iter().find_map(|provider| {
            provider
                .models
                .iter()
                .find(|model| id == Self::model_focus(&provider.id, &model.id).0)
                .map(|model| (provider.id.clone(), model.id.clone()))
        });
        let Some((provider, model)) = selection else {
            return PageOutcome::default();
        };
        PageOutcome::send(ClientMessage::ModelSet { provider, model }, true)
    }
}

pub enum InputPage {
    Settings(SettingsState),
    Login(LoginState),
    Model(ModelPage),
    Theme(ThemePage),
    Resume(ResumePage),
}

pub struct InputPageSession {
    pub page: InputPage,
    pub focus: FocusState,
    pub viewport: ViewportState,
}

impl InputPageSession {
    pub fn settings(state: SettingsState) -> Self {
        let mut page = Self::new(InputPage::Settings(state));
        page.rebuild_focus();
        page
    }

    pub fn login() -> Self {
        let mut page = Self::new(InputPage::Login(LoginState::default()));
        page.rebuild_focus();
        page
    }

    pub fn model() -> Self {
        Self::new(InputPage::Model(ModelPage::loading()))
    }

    pub fn resume() -> Self {
        Self::new(InputPage::Resume(ResumePage::loading()))
    }

    pub fn theme(files: &[ThemeFile], current: &str) -> Self {
        let mut page = Self::new(InputPage::Theme(ThemePage::from_files(files, current)));
        page.rebuild_focus();
        page
    }

    fn new(page: InputPage) -> Self {
        Self {
            page,
            focus: FocusState::default(),
            viewport: ViewportState::default(),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, config: &mut Config) -> PageOutcome {
        let editing = match &self.page {
            InputPage::Settings(settings) => settings.editing.is_some(),
            InputPage::Login(login) => login.editing.is_some(),
            InputPage::Resume(_) => true,
            InputPage::Model(_) | InputPage::Theme(_) => false,
        };
        if !editing {
            if let Some(direction) = direction_from_key(key) {
                if self.focus.move_in(direction) {
                    self.sync_page_from_focus();
                    return PageOutcome::default();
                }
            }
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
                && matches!(key.code, KeyCode::Char('h' | 'j' | 'k' | 'l'))
            {
                return PageOutcome::default();
            }
        }

        let outcome = match &mut self.page {
            InputPage::Settings(settings) => match settings.handle_key(key, config) {
                SettingsAction::None => PageOutcome::default(),
                SettingsAction::Changed => PageOutcome {
                    close: false,
                    effects: vec![PageEffect::ConfigChanged],
                },
                SettingsAction::Exit => PageOutcome::close(),
            },
            InputPage::Login(login) => match login.handle_key(key) {
                LoginAction::None => PageOutcome::default(),
                LoginAction::Exit => PageOutcome::close(),
                LoginAction::Send(message) => PageOutcome::send(message, false),
            },
            InputPage::Model(model) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    PageOutcome::close()
                } else if key.code == KeyCode::Enter {
                    model.activate(&mut self.focus)
                } else {
                    PageOutcome::default()
                }
            }
            InputPage::Theme(theme) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    PageOutcome::close()
                } else if key.code == KeyCode::Enter {
                    let Some(id) = self.focus.current.as_ref() else {
                        return PageOutcome::default();
                    };
                    let Some(name) = id.0.strip_prefix("theme:") else {
                        return PageOutcome::default();
                    };
                    if theme.themes.iter().any(|item| item.name == name) {
                        config.theme = name.to_owned();
                        PageOutcome {
                            close: true,
                            effects: vec![PageEffect::ConfigChanged],
                        }
                    } else {
                        PageOutcome::default()
                    }
                } else {
                    PageOutcome::default()
                }
            }
            InputPage::Resume(resume) => resume.handle_key(key),
        };
        if matches!(self.page, InputPage::Settings(_) | InputPage::Login(_)) {
            self.rebuild_focus();
        }
        outcome
    }

    pub fn apply_modes(&mut self, modes: Vec<String>) {
        if let InputPage::Settings(settings) = &mut self.page {
            settings.modes = modes;
        }
    }

    pub fn apply_login(&mut self, view: LoginView) {
        if let InputPage::Login(login) = &mut self.page {
            login.apply(view);
            self.rebuild_focus();
        }
    }

    pub fn apply_sessions(&mut self, sessions: Vec<SessionInfo>, titles_pending: bool) {
        if let InputPage::Resume(resume) = &mut self.page {
            resume.apply_sessions(sessions, titles_pending);
        }
    }

    pub fn apply_model(
        &mut self,
        providers: Vec<ModelProviderInfo>,
        current: Option<(String, String)>,
    ) {
        if let InputPage::Model(model) = &mut self.page {
            model.apply_catalog(providers, current, &mut self.focus);
        }
    }

    pub fn paste(&mut self, text: &str) -> bool {
        match &mut self.page {
            InputPage::Settings(settings) => {
                if let Some(crate::settings::Edit::Input { buf }) = &mut settings.editing {
                    buf.push_str(text);
                    true
                } else {
                    false
                }
            }
            InputPage::Login(login) => {
                if let Some(buf) = &mut login.editing {
                    buf.push_str(text);
                    true
                } else {
                    false
                }
            }
            InputPage::Resume(resume) => {
                resume.query.push_str(text);
                resume.sel = 0;
                true
            }
            _ => false,
        }
    }

    pub fn rebuild_focus(&mut self) {
        let desired = self.page_focus_id();
        let nodes = match &self.page {
            InputPage::Settings(settings) => settings_focus_nodes(settings),
            InputPage::Login(login) => login_focus_nodes(login),
            InputPage::Model(model) => {
                model.rebuild_focus(&mut self.focus);
                return;
            }
            InputPage::Theme(theme) => {
                let ids: Vec<FocusId> = theme
                    .themes
                    .iter()
                    .map(|item| FocusId::new(format!("theme:{}", item.name)))
                    .collect();
                linear_focus_nodes(&ids, false)
            }
            InputPage::Resume(_) => Vec::new(),
        };
        self.focus.replace(nodes);
        if let Some(desired) = desired {
            self.focus.set(desired);
        }
        self.sync_page_from_focus();
    }

    fn page_focus_id(&self) -> Option<FocusId> {
        match &self.page {
            InputPage::Settings(settings) => settings_focus_id(settings),
            InputPage::Login(login) => login_focus_targets(login)
                .into_iter()
                .find_map(|(id, pos)| (pos == login.pos).then_some(id)),
            InputPage::Model(_) => self.focus.current.clone(),
            InputPage::Theme(theme) => self
                .focus
                .current
                .clone()
                .or_else(|| Some(FocusId::new(format!("theme:{}", theme.current)))),
            InputPage::Resume(_) => None,
        }
    }

    fn sync_page_from_focus(&mut self) {
        let Some(id) = self.focus.current.as_ref() else {
            return;
        };
        match &mut self.page {
            InputPage::Settings(settings) => {
                for index in 0..CATEGORIES.len() {
                    if *id == settings_tab_focus(index) {
                        settings.focus_tabs = true;
                        settings.tab_cursor = index;
                        return;
                    }
                }
                for (index, item) in items_in(settings.category).iter().enumerate() {
                    if *id == settings_item_focus(settings.category, item.label) {
                        settings.focus_tabs = false;
                        settings.pos[settings.category] = index;
                        return;
                    }
                }
            }
            InputPage::Login(login) => {
                if let Some((_, pos)) = login_focus_targets(login)
                    .into_iter()
                    .find(|(candidate, _)| candidate == id)
                {
                    login.pos = pos;
                }
            }
            InputPage::Model(_) | InputPage::Theme(_) | InputPage::Resume(_) => {}
        }
    }
}

fn linear_focus_nodes(ids: &[FocusId], horizontal: bool) -> Vec<FocusNode> {
    ids.iter()
        .enumerate()
        .map(|(index, id)| {
            let previous = index.checked_sub(1).and_then(|i| ids.get(i)).cloned();
            let next = ids.get(index + 1).cloned();
            let mut node = FocusNode::new(id.clone());
            if horizontal {
                node.left = previous.clone();
                node.right = next.clone();
            }
            node.up = previous;
            node.down = next;
            node
        })
        .collect()
}

fn settings_tab_focus(index: usize) -> FocusId {
    FocusId::new(format!("settings:tab:{index}"))
}

fn settings_item_focus(category: usize, label: &str) -> FocusId {
    FocusId::new(format!("settings:item:{category}:{label}"))
}

fn settings_focus_id(settings: &SettingsState) -> Option<FocusId> {
    if settings.focus_tabs {
        return Some(settings_tab_focus(settings.tab_cursor));
    }
    settings
        .current_item()
        .filter(|item| item.kind != ItemKind::ReadOnly)
        .map(|item| settings_item_focus(settings.category, item.label))
}

fn settings_focus_nodes(settings: &SettingsState) -> Vec<FocusNode> {
    let mut nodes = Vec::new();
    let active_items: Vec<FocusId> = items_in(settings.category)
        .into_iter()
        .filter(|item| item.kind != ItemKind::ReadOnly)
        .map(|item| settings_item_focus(settings.category, item.label))
        .collect();
    for index in 0..CATEGORIES.len() {
        let mut node = FocusNode::new(settings_tab_focus(index));
        node.left = Some(settings_tab_focus(
            (index + CATEGORIES.len() - 1) % CATEGORIES.len(),
        ));
        node.right = Some(settings_tab_focus((index + 1) % CATEGORIES.len()));
        if index == settings.category {
            node.down = active_items.first().cloned();
        }
        nodes.push(node);
    }
    for (index, id) in active_items.iter().enumerate() {
        let mut node = FocusNode::new(id.clone());
        node.left = Some(settings_tab_focus(settings.category));
        node.right = Some(settings_tab_focus(settings.category));
        node.up = index
            .checked_sub(1)
            .and_then(|i| active_items.get(i))
            .cloned()
            .or_else(|| Some(settings_tab_focus(settings.category)));
        node.down = active_items.get(index + 1).cloned();
        nodes.push(node);
    }
    nodes
}

fn login_focus_targets(login: &LoginState) -> Vec<(FocusId, usize)> {
    match &login.page {
        LoginPage::Menu => ["api-key", "proxy"]
            .into_iter()
            .enumerate()
            .map(|(pos, name)| (FocusId::new(format!("login:menu:{name}")), pos))
            .collect(),
        LoginPage::Providers => login
            .providers
            .iter()
            .enumerate()
            .filter(|(_, provider)| provider.api_key_writable)
            .map(|(pos, provider)| (FocusId::new(format!("login:provider:{}", provider.id)), pos))
            .collect(),
        LoginPage::ProxyList => login
            .proxies
            .iter()
            .enumerate()
            .map(|(pos, proxy)| (FocusId::new(format!("login:proxy:{}", proxy.id)), pos))
            .chain(std::iter::once((
                FocusId::new("login:proxy:new"),
                login.proxies.len(),
            )))
            .collect(),
        LoginPage::ProxyForm => (0..=PROXY_SAVE_ROW)
            .map(|pos| (FocusId::new(format!("login:proxy-form:{pos}")), pos))
            .collect(),
        LoginPage::ProxyDelete { id, .. } => vec![
            (FocusId::new(format!("login:proxy-delete:{id}:cancel")), 0),
            (FocusId::new(format!("login:proxy-delete:{id}:delete")), 1),
        ],
        LoginPage::ApiKey { .. } => Vec::new(),
    }
}

fn login_focus_nodes(login: &LoginState) -> Vec<FocusNode> {
    let ids: Vec<FocusId> = login_focus_targets(login)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    linear_focus_nodes(&ids, matches!(login.page, LoginPage::ProxyDelete { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn focus_reconciles_by_stable_id() {
        let a = FocusId::new("a");
        let b = FocusId::new("b");
        let mut first = FocusNode::new(a.clone());
        first.down = Some(b.clone());
        let mut second = FocusNode::new(b.clone());
        second.up = Some(a.clone());
        let mut focus = FocusState::default();
        focus.replace(vec![first, second]);
        focus.move_in(Direction::Down);
        assert!(focus.is(&b));
        focus.replace(vec![FocusNode::new(b.clone()), FocusNode::new(a)]);
        assert!(focus.is(&b), "stable id survives reordering");
        focus.replace(vec![FocusNode::new(FocusId::new("c"))]);
        assert!(
            focus.is(&FocusId::new("c")),
            "missing focus falls back safely"
        );
    }

    #[test]
    fn directional_move_skips_disabled_targets() {
        let a = FocusId::new("a");
        let b = FocusId::new("b");
        let c = FocusId::new("c");
        let mut first = FocusNode::new(a.clone());
        first.down = Some(b.clone());
        let mut disabled = FocusNode::new(b);
        disabled.enabled = false;
        disabled.down = Some(c.clone());
        let last = FocusNode::new(c.clone());
        let mut focus = FocusState::default();
        focus.replace(vec![first, disabled, last]);
        focus.move_in(Direction::Down);
        assert!(focus.is(&c));
    }

    #[test]
    fn text_editor_keeps_vim_letters() {
        let mut editor = TextEditor {
            buf: String::new(),
            secret: false,
        };
        for character in "hjkl".chars() {
            assert_eq!(
                handle_text_editor(&mut editor, &key(KeyCode::Char(character))),
                TextEditResult::Continue
            );
        }
        assert_eq!(editor.buf, "hjkl");
        assert_eq!(
            handle_text_editor(&mut editor, &key(KeyCode::Enter)),
            TextEditResult::Confirm("hjkl".into())
        );
        let mut cancelled = TextEditor {
            buf: "draft".into(),
            secret: true,
        };
        assert_eq!(
            handle_text_editor(&mut cancelled, &key(KeyCode::Esc)),
            TextEditResult::Cancel
        );
    }

    #[test]
    fn viewport_is_bounded() {
        let mut viewport = ViewportState::default();
        viewport.ensure_visible(9, 3, 10);
        assert_eq!(viewport.start, 7);
        viewport.ensure_visible(1, 3, 10);
        assert_eq!(viewport.start, 1);
        viewport.ensure_visible(99, 0, 0);
        assert_eq!(viewport.start, 0);
    }

    #[test]
    fn arrows_and_vim_keys_normalize_to_the_same_direction() {
        assert_eq!(
            direction_from_key(&key(KeyCode::Left)),
            Some(Direction::Left)
        );
        assert_eq!(
            direction_from_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
            None,
            "global shortcuts are not Vim navigation"
        );
        assert_eq!(
            direction_from_key(&key(KeyCode::Char('h'))),
            Some(Direction::Left)
        );
        assert_eq!(
            direction_from_key(&key(KeyCode::Down)),
            Some(Direction::Down)
        );
        assert_eq!(
            direction_from_key(&key(KeyCode::Char('j'))),
            Some(Direction::Down)
        );
    }

    #[test]
    fn settings_and_login_use_the_shared_focus_graph() {
        let mut config = Config::default();
        let mut settings = InputPageSession::settings(SettingsState::default());
        let initial_focus = settings.focus.current.clone();
        settings.handle_key(
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
            &mut config,
        );
        assert_eq!(settings.focus.current, initial_focus);
        settings.handle_key(&key(KeyCode::Left), &mut config);
        assert!(settings
            .focus
            .current
            .as_ref()
            .is_some_and(|id| id.0 == "settings:tab:0"));
        settings.handle_key(&key(KeyCode::Right), &mut config);
        settings.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(matches!(settings.page, InputPage::Settings(ref state) if state.category == 1));
        assert!(settings
            .focus
            .current
            .as_ref()
            .is_some_and(|id| id.0.starts_with("settings:item:1:")));

        let mut login = InputPageSession::login();
        login.handle_key(&key(KeyCode::Enter), &mut config);
        login.apply_login(LoginView {
            providers: vec![
                crate::protocol::ProviderInfo {
                    id: "readonly".into(),
                    name: "Read only".into(),
                    api_key_configured: true,
                    api_key_writable: false,
                    api_key_source: Some("env".into()),
                    api_key_hint: None,
                },
                crate::protocol::ProviderInfo {
                    id: "writable".into(),
                    name: "Writable".into(),
                    api_key_configured: false,
                    api_key_writable: true,
                    api_key_source: None,
                    api_key_hint: None,
                },
            ],
            ..Default::default()
        });
        assert!(login
            .focus
            .current
            .as_ref()
            .is_some_and(|id| id.0 == "login:provider:writable"));
    }

    #[test]
    fn model_catalog_keeps_stable_focus_after_reordering() {
        let provider = |id: &str, model: &str| ModelProviderInfo {
            id: id.into(),
            name: id.into(),
            models: vec![crate::protocol::ModelInfo {
                id: model.into(),
                name: model.into(),
                description: None,
            }],
        };
        let mut page = ModelPage::loading();
        let mut focus = FocusState::default();
        page.apply_catalog(
            vec![provider("a", "a1"), provider("b", "b1")],
            Some(("b".into(), "b1".into())),
            &mut focus,
        );
        focus.set(ModelPage::model_focus("b", "b1"));
        page.apply_catalog(
            vec![provider("b", "b1"), provider("a", "a1")],
            Some(("b".into(), "b1".into())),
            &mut focus,
        );
        assert!(focus.is(&ModelPage::model_focus("b", "b1")));
    }

    #[test]
    fn empty_model_catalog_has_no_fake_focus() {
        let mut page = ModelPage::loading();
        let mut focus = FocusState::default();
        page.apply_catalog(Vec::new(), None, &mut focus);
        assert!(focus.current.is_none());
        assert!(!page.loading);
    }

    #[test]
    fn resume_page_filters_progressive_rows_and_attaches_selection() {
        let sessions = || {
            vec![
                SessionInfo {
                    id: "s1".into(),
                    title: "Rust 修复".into(),
                    live: true,
                    created_at: 2,
                },
                SessionInfo {
                    id: "s2".into(),
                    title: "文档整理".into(),
                    live: false,
                    created_at: 1,
                },
            ]
        };
        let mut page = InputPageSession::resume();
        page.apply_sessions(sessions(), true);
        let mut config = Config::default();
        page.handle_key(&key(KeyCode::Char('文')), &mut config);
        assert!(matches!(
            &page.page,
            InputPage::Resume(resume)
                if resume.filtered_indices() == vec![1] && resume.query == "文"
        ));

        // A later title-enriched frame keeps the selected stable session id.
        page.apply_sessions(sessions(), false);
        let outcome = page.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(outcome.close);
        assert!(matches!(
            outcome.effects.as_slice(),
            [PageEffect::Send(ClientMessage::Attach { session_id })] if session_id == "s2"
        ));
    }

    #[test]
    fn resume_search_accepts_hjkl_as_text_and_arrows_navigate() {
        let mut page = InputPageSession::resume();
        page.apply_sessions(
            vec![
                SessionInfo {
                    id: "hjkl-one".into(),
                    title: String::new(),
                    live: false,
                    created_at: 2,
                },
                SessionInfo {
                    id: "hjkl-two".into(),
                    title: String::new(),
                    live: false,
                    created_at: 1,
                },
            ],
            false,
        );
        let mut config = Config::default();
        for character in "hjkl".chars() {
            page.handle_key(&key(KeyCode::Char(character)), &mut config);
        }
        page.handle_key(&key(KeyCode::Down), &mut config);
        assert!(matches!(
            &page.page,
            InputPage::Resume(resume) if resume.query == "hjkl" && resume.sel == 1
        ));
    }

    #[test]
    fn model_activation_sends_selection_and_closes() {
        let mut session = InputPageSession::model();
        session.apply_model(
            vec![ModelProviderInfo {
                id: "p".into(),
                name: "Provider".into(),
                models: vec![crate::protocol::ModelInfo {
                    id: "m".into(),
                    name: "Model".into(),
                    description: None,
                }],
            }],
            Some(("p".into(), "m".into())),
        );
        session.focus.set(ModelPage::model_focus("p", "m"));
        let mut config = Config::default();
        let outcome = session.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(outcome.close);
        assert!(matches!(
            outcome.effects.as_slice(),
            [PageEffect::Send(ClientMessage::ModelSet { provider, model })]
                if provider == "p" && model == "m"
        ));
    }

    #[test]
    fn theme_activation_changes_config_and_requests_persistence() {
        let files = vec![ThemeFile::from_theme("ferra", crate::theme::Theme::ferra())];
        let mut session = InputPageSession::theme(&files, "ferra");
        let mut config = Config::default();
        let outcome = session.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(outcome.close);
        assert_eq!(config.theme, "ferra");
        assert!(matches!(
            outcome.effects.as_slice(),
            [PageEffect::ConfigChanged]
        ));
    }
}
