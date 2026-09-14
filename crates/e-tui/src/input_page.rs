//! Unified configuration pages that replace the ordinary input bar.
//!
//! The closed page roster keeps domain behavior typed while this module owns
//! the common lifecycle, focus navigation, text editing, viewport anchoring,
//! and side-effect boundary used by the main loop.

use crate::key_mapping::{
    Action, MappedKey,
    MappedKey::{Command, Text},
    Scope,
};
#[cfg(test)]
pub use crate::page_core::{direction_from_key, handle_text_editor};
#[cfg(test)]
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

use crate::page_core::linear_focus_nodes;
pub use crate::page_core::{
    direction_from_input, Direction, FocusId, FocusNode, FocusState, PageEffect, PageOutcome,
    TextEditResult, TextEditor, ViewportState,
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

#[path = "input_page/catalog.rs"]
mod catalog_pages;
pub use catalog_pages::{EffortPage, ModelPage, ResumePage};

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

    pub fn authentication(provider_ref: Option<String>, logout: bool) -> Self {
        let mut page = Self::new(InputPage::Login(LoginState::native_loading(
            provider_ref,
            logout,
        )));
        page.rebuild_focus();
        page
    }

    pub fn model() -> Self {
        Self::new(InputPage::Model(ModelPage::loading()))
    }

    pub fn compaction_model() -> Self {
        let mut model = ModelPage::loading();
        model.for_compaction = true;
        Self::new(InputPage::Model(model))
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

    pub fn key_scope(&self) -> Scope {
        match &self.page {
            InputPage::Settings(settings) => settings.key_scope(),
            InputPage::Login(login) => login.key_scope(),
            InputPage::Resume(_) => Scope::PageResume,
            InputPage::Question(question) if question.is_free_text() => Scope::PageQuestionEdit,
            InputPage::Question(_) => Scope::PageQuestion,
            _ => Scope::Page,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent, config: &mut Config) -> PageOutcome {
        let key = if matches!(self.page, InputPage::Model(_)) {
            config.key_mapping.model_input(key)
        } else {
            config.key_mapping.input(self.key_scope(), key)
        };
        if let Some(direction) = direction_from_input(key) {
            if self.focus.move_in(direction) {
                self.sync_page_from_focus();
                return PageOutcome::default();
            }
        }

        let outcome = match &mut self.page {
            InputPage::Settings(settings) => match settings.handle_input(key, config) {
                SettingsAction::None => PageOutcome::default(),
                SettingsAction::Changed => PageOutcome {
                    close: false,
                    effects: vec![PageEffect::ConfigChanged],
                },
                SettingsAction::Exit => PageOutcome::close(),
            },
            InputPage::Login(login) => match login.handle_input(key) {
                LoginAction::None => PageOutcome::default(),
                LoginAction::Exit => PageOutcome::close(),
                LoginAction::Send(message) => PageOutcome::send(message, false),
                LoginAction::Copy(value) => PageOutcome {
                    close: false,
                    effects: vec![PageEffect::WriteClipboard(value)],
                },
                LoginAction::Cancel => PageOutcome {
                    close: true,
                    effects: vec![PageEffect::Send(AgentRequest::AuthCancel)],
                },
            },
            InputPage::Model(model) => match key {
                Command(Action::Back | Action::Close) => PageOutcome::close(),
                Command(Action::Confirm) => model.activate(&mut self.focus),
                MappedKey::MarkModel(letter) => model.mark(letter, &self.focus, config),
                MappedKey::SelectModel(letter) => model.select_mark(letter, config),
                _ => PageOutcome::default(),
            },
            InputPage::Effort(effort) => {
                if matches!(key, Command(Action::Back | Action::Close)) {
                    PageOutcome::close()
                } else if key == Command(Action::Confirm) {
                    effort.activate(&mut self.focus)
                } else {
                    PageOutcome::default()
                }
            }
            InputPage::Theme(theme) => {
                if matches!(key, Command(Action::Back | Action::Close)) {
                    PageOutcome::close()
                } else if key == Command(Action::Confirm) {
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
            InputPage::Resume(resume) => resume.handle_input(key),
            InputPage::Question(question) => match key {
                Command(Action::Cancel) => PageOutcome::send(
                    AgentRequest::CancelQuestions {
                        request_id: question.rpc_id.clone(),
                    },
                    true,
                ),
                Command(Action::PreviousQuestion) => {
                    question.step_question(-1);
                    PageOutcome::default()
                }
                Command(Action::NextQuestion) => {
                    question.step_question(1);
                    PageOutcome::default()
                }
                Command(Action::ToggleOption) if !question.is_free_text() => {
                    question.toggle_selection();
                    PageOutcome::default()
                }
                Command(Action::Confirm) => question
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
                Command(Action::DeleteBackward) if question.is_free_text() => {
                    question.backspace();
                    PageOutcome::default()
                }
                Text(character) if question.is_free_text() => {
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

    pub fn apply_auth_catalog(
        &mut self,
        providers: Vec<crate::agent::AuthProvider>,
        provider_ref: Option<String>,
        logout: bool,
        error: Option<String>,
    ) {
        if let InputPage::Login(login) = &mut self.page {
            login.apply_auth_catalog(providers, provider_ref, logout, error);
            self.rebuild_focus();
        }
    }

    pub fn start_auth(&mut self, flow_id: String) {
        if let InputPage::Login(login) = &mut self.page {
            login.start_auth(flow_id);
            self.rebuild_focus();
        }
    }

    pub fn apply_auth_prompt(&mut self, prompt: crate::agent::AuthPrompt) {
        if let InputPage::Login(login) = &mut self.page {
            login.apply_auth_prompt(prompt);
            self.rebuild_focus();
        }
    }

    pub fn withdraw_auth_prompt(&mut self, flow_id: &str, prompt_id: &str) {
        if let InputPage::Login(login) = &mut self.page {
            login.withdraw_auth_prompt(flow_id, prompt_id);
            self.rebuild_focus();
        }
    }

    pub fn apply_auth_notice(&mut self, notice: crate::agent::AuthNotice) {
        if let InputPage::Login(login) = &mut self.page {
            login.apply_auth_notice(notice);
            self.rebuild_focus();
        }
    }

    pub fn finish_auth(
        &mut self,
        flow_id: &str,
        outcome: crate::agent::AuthOutcomeKind,
        message: String,
    ) {
        if let InputPage::Login(login) = &mut self.page {
            login.finish_auth(flow_id, outcome, message);
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
        LoginPage::NativeProviders
        | LoginPage::NativeMethods { .. }
        | LoginPage::NativeLogout { .. }
        | LoginPage::NativePrompt(_) => (0..login.row_count())
            .filter_map(|pos| Some((FocusId::new(login.row_focus_id(pos)?), pos)))
            .collect(),
        LoginPage::ApiKey { .. } => Vec::new(),
        LoginPage::NativeWaiting { .. } | LoginPage::NativeOutcome { .. } => Vec::new(),
    }
}

fn login_focus_nodes(login: &LoginState) -> Vec<FocusNode> {
    let ids: Vec<FocusId> = login_focus_targets(login)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    linear_focus_nodes(
        &ids,
        matches!(
            login.page,
            LoginPage::ProxyDelete { .. } | LoginPage::NativeLogout { .. }
        ),
    )
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
                context_window: None,
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
                context_window: None,
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
                    modified_at: None,
                },
                SessionSummary {
                    id: second.into(),
                    title: format!("Title {second}"),
                    live: false,
                    created_at: 1,
                    modified_at: None,
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
                    modified_at: None,
                },
                SessionSummary {
                    id: "s2".into(),
                    title: "文档整理".into(),
                    live: false,
                    created_at: 1,
                    modified_at: None,
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
                    modified_at: None,
                },
                SessionSummary {
                    id: "hjkl-two".into(),
                    title: String::new(),
                    live: false,
                    created_at: 1,
                    modified_at: None,
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

    fn marked_model_page() -> InputPageSession {
        let mut page = InputPageSession::model();
        page.apply_model(
            ["p", "q"]
                .into_iter()
                .map(|id| ModelProvider {
                    id: id.into(),
                    name: id.into(),
                    models: vec![crate::agent::ModelDescriptor {
                        id: "m".into(),
                        name: "Model".into(),
                        description: None,
                        context_window: None,
                        reasoning: None,
                    }],
                })
                .collect(),
            Some(("p".into(), "m".into())),
        );
        page
    }

    #[test]
    fn model_marks_toggle_reopen_and_select_across_provider_refreshes() {
        let mut config = Config::default();
        let mut page = marked_model_page();
        let mark = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        for expected in [Some('a'), None, Some('a')] {
            let outcome = page.handle_key(&mark, &mut config);
            assert!(!outcome.close);
            assert!(matches!(
                outcome.effects.as_slice(),
                [PageEffect::ConfigChanged]
            ));
            assert_eq!(config.model_marks.letter("p", "m"), expected);
        }
        page.handle_key(&key(KeyCode::Esc), &mut config);
        config = Config::from_user_toml(&toml::to_string(&config).unwrap()).unwrap();
        let mut page = marked_model_page();
        let InputPage::Model(model) = &page.page else {
            panic!()
        };
        let providers = model.providers.iter().rev().cloned().collect();
        page.apply_model(providers, Some(("q".into(), "m".into())));
        page.focus.set(ModelPage::provider_focus("q"));
        page.handle_key(&key(KeyCode::Enter), &mut config);
        assert!(page.focus.is(&ModelPage::model_focus("q", "m")));
        let outcome = page.handle_key(&key(KeyCode::Char('a')), &mut config);
        assert!(outcome.close);
        assert!(
            matches!(outcome.effects.as_slice(), [PageEffect::Send(AgentRequest::ModelSet {
            provider, model, reasoning_effort: None,
        })] if provider == "p" && model == "m")
        );
    }

    #[test]
    fn model_marks_ignore_missing_targets_and_respect_reloaded_bindings() {
        let mut config = Config::default();
        let mut page = InputPageSession::model();
        let mark = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SHIFT);
        assert!(page.handle_key(&mark, &mut config).effects.is_empty());
        page.apply_model(Vec::new(), None);
        assert!(page.handle_key(&mark, &mut config).effects.is_empty());
        page = marked_model_page();
        page.focus.set(ModelPage::provider_focus("p"));
        assert!(page.handle_key(&mark, &mut config).effects.is_empty());
        page.focus.set(ModelPage::model_focus("p", "m"));
        for letter in "hjklq".chars() {
            assert!(page
                .handle_key(
                    &KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT),
                    &mut config
                )
                .effects
                .is_empty());
            assert_eq!(config.model_marks.get(letter), None);
        }
        page.handle_key(&mark, &mut config);
        config.key_mapping =
            crate::key_mapping::KeyMapping::from_user_toml("[page]\nmove_down='shift-a'").unwrap();
        let outcome = page.handle_key(&key(KeyCode::Char('a')), &mut config);
        assert!(!outcome.close && outcome.effects.is_empty());
        page.handle_key(&mark, &mut config);
        assert_eq!(config.model_marks.letter("p", "m"), Some('a'));
        config.key_mapping = crate::key_mapping::KeyMapping::default();
        let InputPage::Model(model) = &page.page else {
            panic!()
        };
        let remaining = model
            .providers
            .iter()
            .filter(|p| p.id == "q")
            .cloned()
            .collect();
        page.apply_model(remaining, Some(("q".into(), "m".into())));
        for letter in ['a', 'z'] {
            let outcome = page.handle_key(&key(KeyCode::Char(letter)), &mut config);
            assert!(!outcome.close && outcome.effects.is_empty());
        }
        assert_eq!(config.model_marks.letter("p", "m"), Some('a'));
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
                    context_window: None,
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
    fn compaction_model_activation_does_not_select_conversation_model() {
        let mut session = InputPageSession::compaction_model();
        session.apply_model(
            vec![ModelProvider {
                id: "p".into(),
                name: "Provider".into(),
                models: vec![crate::agent::ModelDescriptor {
                    id: "m".into(),
                    name: "Small".into(),
                    description: None,
                    context_window: None,
                    reasoning: None,
                }],
            }],
            None,
        );
        session.focus.set(ModelPage::model_focus("p", "m"));
        let outcome = session.handle_key(&key(KeyCode::Enter), &mut Config::default());
        assert!(outcome.close);
        assert!(
            matches!(outcome.effects.as_slice(), [PageEffect::Send(AgentRequest::Command { line, images })] if line == "/compact set-model p/m" && images.is_empty())
        );
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
                    context_window: None,
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
                    context_window: None,
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
