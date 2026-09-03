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
    action::AgentRequest,
    agent::{ModelProvider, ModelSelection, ReasoningEffort, SessionSummary},
    catalog::CatalogModel,
    config::Config,
    login::{LoginAction, LoginState, LoginView, Page as LoginPage, PROXY_SAVE_ROW},
    question::QuestionBatch,
    settings::{items_in, ItemKind, SettingsAction, SettingsState},
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
    pub sessions: Vec<SessionSummary>,
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

    pub fn apply_sessions(&mut self, sessions: Vec<SessionSummary>, titles_pending: bool) {
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
                        AgentRequest::Attach {
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
    pub providers: Vec<ModelProvider>,
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
        providers: Vec<ModelProvider>,
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

    pub fn active_models(&self) -> &[crate::agent::ModelDescriptor] {
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
        PageOutcome::send(
            AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort: None,
            },
            true,
        )
    }
}

pub struct EffortPage {
    pub efforts: Vec<ReasoningEffort>,
    pub current: Option<ModelSelection>,
    pub default_effort: Option<String>,
    pub loading: bool,
    pub unavailable: bool,
}

impl EffortPage {
    pub fn loading() -> Self {
        Self {
            efforts: Vec::new(),
            current: None,
            default_effort: None,
            loading: true,
            unavailable: false,
        }
    }

    /// Populate from the sole catalog owner: only the exact current route's
    /// adapter-declared efforts become selectable. An absent route or empty
    /// effort list becomes an unavailable state with no fake focus.
    pub fn apply_catalog(&mut self, catalog: &CatalogModel) {
        self.efforts.clear();
        self.current = catalog.current_model.clone();
        self.default_effort = None;
        self.loading = false;
        self.unavailable = true;
        if let Some(reasoning) = catalog.current_model_reasoning() {
            self.efforts = reasoning.efforts.clone();
            self.default_effort = reasoning.default_effort.clone();
            self.unavailable = self.efforts.is_empty();
        }
    }

    fn effort_focus(id: &str) -> FocusId {
        FocusId::new(format!("effort:{id}"))
    }

    pub fn rebuild_focus(&self, focus: &mut FocusState) {
        // Capture whether this is a fresh open so a catalog refresh preserves
        // the user's in-page navigation instead of snapping back to the
        // preferred effort.
        let was_empty = focus.current.is_none();
        let mut nodes = Vec::new();
        for (index, effort) in self.efforts.iter().enumerate() {
            let mut node = FocusNode::new(Self::effort_focus(&effort.id));
            node.up = index
                .checked_sub(1)
                .and_then(|i| self.efforts.get(i))
                .map(|item| Self::effort_focus(&item.id));
            node.down = self
                .efforts
                .get(index + 1)
                .map(|item| Self::effort_focus(&item.id));
            nodes.push(node);
        }
        focus.replace(nodes);
        if was_empty {
            let preferred = self
                .current
                .as_ref()
                .and_then(|current| current.reasoning_effort.clone())
                .or_else(|| self.default_effort.clone());
            let target = preferred
                .and_then(|id| self.efforts.iter().find(|effort| effort.id == id))
                .or_else(|| self.efforts.first())
                .map(|effort| Self::effort_focus(&effort.id));
            if let Some(target) = target {
                focus.set(target);
            }
        }
    }

    fn activate(&mut self, focus: &mut FocusState) -> PageOutcome {
        let Some(id) = focus.current.as_ref().map(|id| id.0.clone()) else {
            return PageOutcome::default();
        };
        let Some(effort_id) = id.strip_prefix("effort:") else {
            return PageOutcome::default();
        };
        let Some(current) = self.current.as_ref() else {
            return PageOutcome::default();
        };
        PageOutcome::send(
            AgentRequest::ModelSet {
                provider: current.provider.clone(),
                model: current.model.clone(),
                reasoning_effort: Some(effort_id.to_owned()),
            },
            true,
        )
    }
}

pub enum InputPage {
    Settings(SettingsState),
    Login(LoginState),
    Model(ModelPage),
    Effort(EffortPage),
    Theme(ThemePage),
    Resume(ResumePage),
    Question(QuestionBatch),
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

    pub fn effort() -> Self {
        Self::new(InputPage::Effort(EffortPage::loading()))
    }

    pub fn resume() -> Self {
        Self::new(InputPage::Resume(ResumePage::loading()))
    }

    pub fn question(batch: QuestionBatch) -> Self {
        let mut page = Self::new(InputPage::Question(batch));
        page.rebuild_focus();
        page
    }

    pub fn question_rpc_id(&self) -> Option<&str> {
        match &self.page {
            InputPage::Question(batch) => Some(batch.rpc_id.as_str()),
            _ => None,
        }
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
            InputPage::Question(question) => question.is_free_text(),
            InputPage::Model(_) | InputPage::Effort(_) | InputPage::Theme(_) => false,
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
            InputPage::Effort(effort) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    PageOutcome::close()
                } else if key.code == KeyCode::Enter {
                    effort.activate(&mut self.focus)
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
            InputPage::Question(question) => match key.code {
                KeyCode::Esc => PageOutcome::send(
                    AgentRequest::CancelQuestions {
                        request_id: question.rpc_id.clone(),
                    },
                    true,
                ),
                KeyCode::Left | KeyCode::Char('h') if !question.is_free_text() => {
                    question.step_question(-1);
                    PageOutcome::default()
                }
                KeyCode::Right | KeyCode::Char('l') if !question.is_free_text() => {
                    question.step_question(1);
                    PageOutcome::default()
                }
                KeyCode::Left => {
                    question.step_question(-1);
                    PageOutcome::default()
                }
                KeyCode::Right => {
                    question.step_question(1);
                    PageOutcome::default()
                }
                KeyCode::Char(' ') if !question.is_free_text() => {
                    question.toggle_selection();
                    PageOutcome::default()
                }
                KeyCode::Enter => question
                    .enter()
                    .map(|answers| {
                        PageOutcome::send(
                            AgentRequest::AnswerQuestions {
                                request_id: question.rpc_id.clone(),
                                answers,
                            },
                            true,
                        )
                    })
                    .unwrap_or_default(),
                KeyCode::Backspace if question.is_free_text() => {
                    question.backspace();
                    PageOutcome::default()
                }
                KeyCode::Char(character)
                    if question.is_free_text()
                        && !character.is_ascii_control()
                        && !key.modifiers.intersects(
                            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                        ) =>
                {
                    question.push_char(character);
                    PageOutcome::default()
                }
                _ => PageOutcome::default(),
            },
        };
        if matches!(
            self.page,
            InputPage::Settings(_) | InputPage::Login(_) | InputPage::Question(_)
        ) && !outcome.close
        {
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

    pub fn apply_sessions(&mut self, sessions: Vec<SessionSummary>, titles_pending: bool) {
        if let InputPage::Resume(resume) = &mut self.page {
            resume.apply_sessions(sessions, titles_pending);
        }
    }

    pub fn apply_model(
        &mut self,
        providers: Vec<ModelProvider>,
        current: Option<(String, String)>,
    ) {
        if let InputPage::Model(model) = &mut self.page {
            model.apply_catalog(providers, current, &mut self.focus);
        }
    }

    pub fn apply_effort(&mut self, catalog: &CatalogModel) {
        if let InputPage::Effort(effort) = &mut self.page {
            effort.apply_catalog(catalog);
            effort.rebuild_focus(&mut self.focus);
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
            InputPage::Question(question) if question.is_free_text() => {
                question.draft.push_str(text);
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
            InputPage::Effort(effort) => {
                effort.rebuild_focus(&mut self.focus);
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
            InputPage::Question(question) => question_focus_nodes(question),
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
            InputPage::Model(_) | InputPage::Effort(_) => self.focus.current.clone(),
            InputPage::Theme(theme) => self
                .focus
                .current
                .clone()
                .or_else(|| Some(FocusId::new(format!("theme:{}", theme.current)))),
            InputPage::Resume(_) => None,
            InputPage::Question(question) => {
                (!question.is_free_text()).then(|| question_option_focus(question, question.sel))
            }
        }
    }

    fn sync_page_from_focus(&mut self) {
        let Some(id) = self.focus.current.as_ref() else {
            return;
        };
        match &mut self.page {
            InputPage::Settings(settings) => {
                for (index, item) in items_in(settings.category).iter().enumerate() {
                    if *id == settings_item_focus(settings.category, item.key) {
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
            InputPage::Question(question) => {
                if let Some(index) =
                    question
                        .current_options()
                        .iter()
                        .enumerate()
                        .find_map(|(index, _)| {
                            (question_option_focus(question, index) == *id).then_some(index)
                        })
                {
                    question.sel = index;
                }
            }
            InputPage::Model(_)
            | InputPage::Effort(_)
            | InputPage::Theme(_)
            | InputPage::Resume(_) => {}
        }
    }
}

fn question_option_focus(question: &QuestionBatch, index: usize) -> FocusId {
    let question_id = question
        .questions
        .get(question.current)
        .map(|item| item.id.as_str())
        .unwrap_or("missing");
    FocusId::new(format!("question:{question_id}:option:{index}"))
}

fn question_focus_nodes(question: &QuestionBatch) -> Vec<FocusNode> {
    let ids = question
        .current_options()
        .iter()
        .enumerate()
        .map(|(index, _)| question_option_focus(question, index))
        .collect::<Vec<_>>();
    ids.iter()
        .enumerate()
        .map(|(index, id)| {
            let mut node = FocusNode::new(id.clone());
            node.up = index.checked_sub(1).and_then(|i| ids.get(i)).cloned();
            node.down = ids.get(index + 1).cloned();
            node
        })
        .collect()
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

fn settings_item_focus(category: usize, key: &str) -> FocusId {
    FocusId::new(format!("settings:item:{category}:{key}"))
}

fn settings_focus_id(settings: &SettingsState) -> Option<FocusId> {
    settings
        .current_item()
        .filter(|item| item.kind != ItemKind::ReadOnly)
        .map(|item| settings_item_focus(settings.category, item.key))
}

fn settings_focus_nodes(settings: &SettingsState) -> Vec<FocusNode> {
    let active_items: Vec<FocusId> = items_in(settings.category)
        .into_iter()
        .filter(|item| item.kind != ItemKind::ReadOnly)
        .map(|item| settings_item_focus(settings.category, item.key))
        .collect();
    active_items
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let mut node = FocusNode::new(id.clone());
            node.up = index
                .checked_sub(1)
                .and_then(|i| active_items.get(i))
                .cloned();
            node.down = active_items.get(index + 1).cloned();
            node
        })
        .collect()
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
        assert!(initial_focus
            .as_ref()
            .is_some_and(|id| id.0.starts_with("settings:item:0:")));
        settings.handle_key(
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
            &mut config,
        );
        assert_eq!(settings.focus.current, initial_focus);

        settings.handle_key(&key(KeyCode::Left), &mut config);
        assert!(matches!(settings.page, InputPage::Settings(ref state) if state.category == 3));
        assert_eq!(
            settings.focus.current, None,
            "the display-only tabs and read-only rows never enter focus"
        );
        settings.handle_key(&key(KeyCode::Right), &mut config);
        settings.handle_key(&key(KeyCode::Right), &mut config);
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
                crate::agent::CredentialProvider {
                    id: "readonly".into(),
                    name: "Read only".into(),
                    api_key_configured: true,
                    api_key_writable: false,
                    api_key_source: Some("env".into()),
                    api_key_hint: None,
                },
                crate::agent::CredentialProvider {
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
        let provider = |id: &str, model: &str| ModelProvider {
            id: id.into(),
            name: id.into(),
            models: vec![crate::agent::ModelDescriptor {
                id: model.into(),
                name: model.into(),
                description: None,
                reasoning: None,
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
    fn localized_page_rebuild_preserves_stable_targets_and_active_edits() {
        let mut config = Config::default();

        let mut settings = InputPageSession::settings(SettingsState::default());
        if let InputPage::Settings(page) = &mut settings.page {
            page.category = 1;
            page.pos[1] = 1; // language
            page.editing = Some(crate::settings::Edit::Choice { cursor: 1 });
        }
        settings.rebuild_focus();
        config.language = crate::Language::SimplifiedChinese;
        settings.rebuild_focus();
        assert_eq!(
            settings.focus.current.as_ref().map(|id| id.0.as_str()),
            Some("settings:item:1:language")
        );
        assert!(
            matches!(
                &settings.page,
                InputPage::Settings(page)
                    if page.editing.as_ref()
                        == Some(&crate::settings::Edit::Choice { cursor: 1 })
            ),
            "the choice cursor is not derived from its translated label"
        );

        let provider = |id: &str| crate::agent::CredentialProvider {
            id: id.into(),
            name: format!("Provider {id}"),
            api_key_configured: false,
            api_key_writable: true,
            api_key_source: None,
            api_key_hint: None,
        };
        let mut login = InputPageSession::login();
        if let InputPage::Login(page) = &mut login.page {
            page.page = LoginPage::Providers;
            page.providers = vec![provider("a"), provider("b")];
            page.pos = 1;
            page.editing = Some("partly typed".into());
        }
        login.rebuild_focus();
        login.apply_login(LoginView {
            providers: vec![provider("b"), provider("a")],
            ..Default::default()
        });
        assert_eq!(
            login.focus.current.as_ref().map(|id| id.0.as_str()),
            Some("login:provider:b")
        );
        assert!(matches!(
            &login.page,
            InputPage::Login(page) if page.editing.as_deref() == Some("partly typed")
        ));

        let model_provider = |id: &str| ModelProvider {
            id: id.into(),
            name: format!("Provider {id}"),
            models: vec![crate::agent::ModelDescriptor {
                id: format!("{id}-model"),
                name: format!("Model {id}"),
                description: None,
                reasoning: None,
            }],
        };
        let mut model = InputPageSession::model();
        model.apply_model(
            vec![model_provider("a"), model_provider("b")],
            Some(("b".into(), "b-model".into())),
        );
        model.focus.set(FocusId::new("model:b:b-model"));
        model.apply_model(
            vec![model_provider("b"), model_provider("a")],
            Some(("b".into(), "b-model".into())),
        );
        assert!(model.focus.is(&FocusId::new("model:b:b-model")));

        let sessions = |first: &str, second: &str| {
            vec![
                SessionSummary {
                    id: first.into(),
                    title: format!("Title {first}"),
                    live: false,
                    created_at: 2,
                },
                SessionSummary {
                    id: second.into(),
                    title: format!("Title {second}"),
                    live: false,
                    created_at: 1,
                },
            ]
        };
        let mut resume = InputPageSession::resume();
        resume.apply_sessions(sessions("s1", "s2"), false);
        if let InputPage::Resume(page) = &mut resume.page {
            page.sel = 1;
        }
        resume.apply_sessions(sessions("s2", "s1"), false);
        assert!(matches!(
            &resume.page,
            InputPage::Resume(page) if page.sessions[page.sel].id == "s2"
        ));

        let mut question = InputPageSession::question(QuestionBatch::new(
            "rpc".into(),
            "session".into(),
            vec![crate::agent::Question {
                id: "question-id".into(),
                question: "Question text".into(),
                header: None,
                options: Some(
                    ["Option A", "Option B"]
                        .into_iter()
                        .map(|label| crate::agent::QuestionOption {
                            label: label.into(),
                            description: None,
                        })
                        .collect(),
                ),
                multi_select: false,
            }],
        ));
        if let InputPage::Question(page) = &mut question.page {
            page.sel = 1;
        }
        question.rebuild_focus();
        assert_eq!(
            question.focus.current.as_ref().map(|id| id.0.as_str()),
            Some("question:question-id:option:1")
        );

        let mut free_text = InputPageSession::question(QuestionBatch::new(
            "rpc".into(),
            "session".into(),
            vec![crate::agent::Question {
                id: "free-text-id".into(),
                question: "Why?".into(),
                header: None,
                options: None,
                multi_select: false,
            }],
        ));
        if let InputPage::Question(page) = &mut free_text.page {
            page.draft = "unfinished answer".into();
        }
        free_text.rebuild_focus();
        assert!(matches!(
            &free_text.page,
            InputPage::Question(page) if page.draft == "unfinished answer"
        ));
    }

    #[test]
    fn resume_page_filters_progressive_rows_and_attaches_selection() {
        let sessions = || {
            vec![
                SessionSummary {
                    id: "s1".into(),
                    title: "Rust 修复".into(),
                    live: true,
                    created_at: 2,
                },
                SessionSummary {
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
            [PageEffect::Send(AgentRequest::Attach { session_id })] if session_id == "s2"
        ));
    }

    #[test]
    fn resume_search_accepts_hjkl_as_text_and_arrows_navigate() {
        let mut page = InputPageSession::resume();
        page.apply_sessions(
            vec![
                SessionSummary {
                    id: "hjkl-one".into(),
                    title: String::new(),
                    live: false,
                    created_at: 2,
                },
                SessionSummary {
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
            vec![ModelProvider {
                id: "p".into(),
                name: "Provider".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "m".into(),
                    name: "Model".into(),
                    description: None,
                    reasoning: None,
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
            [PageEffect::Send(AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort: None,
            })] if provider == "p" && model == "m"
        ));
    }

    #[test]
    fn effort_activation_sends_the_full_selection_and_closes() {
        let mut session = InputPageSession::effort();
        let catalog = CatalogModel {
            current_model: Some(ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: None,
            }),
            model_providers: vec![ModelProvider {
                id: "openai".into(),
                name: "OpenAI".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "gpt".into(),
                    name: "GPT".into(),
                    description: None,
                    reasoning: Some(crate::agent::ModelReasoning {
                        // Default is deliberately NOT first so the pre-focus
                        // assertion below cannot pass by coincidence.
                        efforts: vec![
                            ReasoningEffort {
                                id: "low".into(),
                                name: "Low".into(),
                                description: None,
                            },
                            ReasoningEffort {
                                id: "high".into(),
                                name: "High".into(),
                                description: None,
                            },
                        ],
                        default_effort: Some("high".into()),
                    }),
                }],
            }],
            ..CatalogModel::default()
        };
        session.apply_effort(&catalog);
        // The adapter default (not the first row) is pre-focused.
        assert_eq!(
            session.focus.current.as_ref().map(|id| id.0.as_str()),
            Some("effort:high")
        );
        // Move up to the first row and submit it.
        session.focus.move_in(crate::page_core::Direction::Up);
        let mut config = Config::default();
        let outcome = session.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(outcome.close);
        assert!(matches!(
            outcome.effects.as_slice(),
            [PageEffect::Send(AgentRequest::ModelSet {
                provider,
                model,
                reasoning_effort: Some(effort),
            })] if provider == "openai" && model == "gpt" && effort == "low"
        ));
    }

    #[test]
    fn effort_page_has_no_fake_focus_when_unavailable() {
        let mut session = InputPageSession::effort();
        let catalog = CatalogModel {
            current_model: Some(ModelSelection {
                provider: "openai".into(),
                model: "plain".into(),
                reasoning_effort: None,
            }),
            model_providers: vec![ModelProvider {
                id: "openai".into(),
                name: "OpenAI".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "plain".into(),
                    name: "Plain".into(),
                    description: None,
                    reasoning: None,
                }],
            }],
            ..CatalogModel::default()
        };
        session.apply_effort(&catalog);
        assert!(session.focus.current.is_none());
        assert!(matches!(
            &session.page,
            InputPage::Effort(page) if page.unavailable
        ));
    }

    #[test]
    fn question_page_uses_h_l_for_questions_and_j_k_for_options() {
        let question = |id: &str, labels: &[&str]| crate::agent::Question {
            id: id.into(),
            question: format!("{id}?"),
            header: None,
            options: Some(
                labels
                    .iter()
                    .map(|label| crate::agent::QuestionOption {
                        label: (*label).into(),
                        description: None,
                    })
                    .collect(),
            ),
            multi_select: false,
        };
        let mut page = InputPageSession::question(QuestionBatch::new(
            "rpc".into(),
            "session".into(),
            vec![
                question("first", &["A", "B"]),
                question("second", &["C", "D"]),
            ],
        ));
        let mut config = Config::default();

        page.handle_key(&key(KeyCode::Char('j')), &mut config);
        assert!(
            matches!(&page.page, InputPage::Question(batch) if batch.current == 0 && batch.sel == 1)
        );
        page.handle_key(&key(KeyCode::Char('l')), &mut config);
        assert!(
            matches!(&page.page, InputPage::Question(batch) if batch.current == 1 && batch.sel == 0)
        );
        page.handle_key(&key(KeyCode::Char('j')), &mut config);
        page.handle_key(&key(KeyCode::Char('h')), &mut config);
        assert!(
            matches!(&page.page, InputPage::Question(batch) if batch.current == 0 && batch.sel == 1)
        );
        page.handle_key(&key(KeyCode::Right), &mut config);
        assert!(
            matches!(&page.page, InputPage::Question(batch) if batch.current == 1 && batch.sel == 1)
        );
        page.handle_key(&key(KeyCode::Char('k')), &mut config);
        assert!(matches!(&page.page, InputPage::Question(batch) if batch.sel == 0));
        page.handle_key(&key(KeyCode::Down), &mut config);
        let submitted = page.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(submitted.close);
        assert!(matches!(
            submitted.effects.as_slice(),
            [PageEffect::Send(AgentRequest::AnswerQuestions { request_id, answers })]
                if request_id == "rpc"
                    && answers[0].selected == ["B"]
                    && answers[1].selected == ["D"]
        ));
    }

    #[test]
    fn question_space_toggles_multi_select_without_leaving_the_question() {
        let mut page = InputPageSession::question(QuestionBatch::new(
            "rpc".into(),
            "session".into(),
            vec![crate::agent::Question {
                id: "many".into(),
                question: "choose".into(),
                header: None,
                options: Some(
                    ["A", "B", "C"]
                        .into_iter()
                        .map(|label| crate::agent::QuestionOption {
                            label: label.into(),
                            description: None,
                        })
                        .collect(),
                ),
                multi_select: true,
            }],
        ));
        let mut config = Config::default();

        page.handle_key(&key(KeyCode::Char('j')), &mut config);
        let selected = page.handle_key(&key(KeyCode::Char(' ')), &mut config);
        assert!(!selected.close);
        assert!(selected.effects.is_empty());
        assert!(matches!(&page.page, InputPage::Question(batch)
            if batch.current == 0 && batch.sel == 1 && batch.is_option_selected(1)));
        page.handle_key(&key(KeyCode::Char('j')), &mut config);
        page.handle_key(&key(KeyCode::Char(' ')), &mut config);
        page.handle_key(&key(KeyCode::Char('k')), &mut config);
        page.handle_key(&key(KeyCode::Char(' ')), &mut config);

        let submitted = page.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(matches!(
            submitted.effects.as_slice(),
            [PageEffect::Send(AgentRequest::AnswerQuestions { answers, .. })]
                if answers[0].selected == ["C"]
        ));
    }

    #[test]
    fn question_free_text_keeps_h_l_as_text() {
        let mut page = InputPageSession::question(QuestionBatch::new(
            "rpc".into(),
            "session".into(),
            vec![crate::agent::Question {
                id: "free".into(),
                question: "why?".into(),
                header: None,
                options: None,
                multi_select: false,
            }],
        ));
        let mut config = Config::default();
        page.handle_key(&key(KeyCode::Char('h')), &mut config);
        page.handle_key(&key(KeyCode::Char('l')), &mut config);
        assert!(matches!(&page.page, InputPage::Question(batch) if batch.draft == "hl"));
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
