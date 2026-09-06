//! Testable runtime inputs, effects, and bridge-message controller.
//!
//! The Tokio loop in `main.rs` owns waiting and concrete I/O. This module
//! mutates typed client state and returns effects that the runner executes only
//! after state guards have been released.

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    command_catalog::NewMode,
    input::{InputAction, InputState},
    input_page::{InputPageSession, PageEffect},
    interaction::{ApprovalCard, PendingPromptQueue},
    login::LoginView,
    question::QuestionBatch,
    runtime::{
        command::{self as runtime_command, LocalCommandContext},
        input::TerminalRoute,
        state::RuntimeState,
    },
    ui::{scroll_lines, scroll_page, transcript_view_height, ScrollState, TerminalSize},
    AgentEvent, AgentRequest, ClipboardPaste, Config, MouseSelection, NoticeState, PaneResizeState,
    PointerEvent, PromptImage, PromptInput, SelectionFrame, Theme, ThemeFile,
};
pub use crate::{DrawPriority, EffectResult, UiAction};

mod agent;
mod effect;
mod input;
mod terminal;

pub enum RuntimeInput {
    Bridge(AgentEvent),
    Terminal(Event),
    EffectCompleted(EffectResult),
    AnimationDeadline,
    FrameDeadline,
}

/// UI-local page transitions consumed synchronously by the controller.
pub enum ControllerAction {
    OpenPage(InputPageSession),
    ClosePage,
}

fn paste_text(
    input: &mut InputState,
    input_page: &mut Option<InputPageSession>,
    text: &str,
) -> bool {
    if text.is_empty() {
        return false;
    }
    if let Some(page) = input_page {
        page.paste(text)
    } else {
        input.paste(text);
        true
    }
}

fn agent_action(request: AgentRequest) -> UiAction {
    UiAction::Agent(request)
}

/// Mutable UI-local state affected by bridge frames. Keeping it separate from
/// RuntimeState makes session-switch behavior explicit without giving the bridge
/// handler ownership of terminal or transport infrastructure. The interaction
/// fields (approval/question/queue) are borrowed from the same InteractionModel
/// the terminal path uses, so bridge frames and key handling mutate the same
/// object — never a transient default.
pub struct RuntimeUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut PendingPromptQueue,
}

#[derive(Default)]
pub struct InputHandlerOutcome {
    pub command: Option<PendingCommand>,
    pub activate_reading: bool,
    pub effects: Vec<UiAction>,
}

pub struct PendingCommand {
    pub line: String,
    pub images: Vec<PromptImage>,
    pub original: PromptInput,
}

pub struct InputPageUiState<'a> {
    pub input_page: &'a mut Option<InputPageSession>,
    pub input: &'a mut InputState,
    pub config: &'a mut Config,
    pub themes: &'a [ThemeFile],
    pub theme: &'a mut Theme,
    pub question: &'a mut Option<String>,
}

pub struct TerminalUiState<'a> {
    pub scroll: &'a mut ScrollState,
    pub input: &'a mut InputState,
    pub input_page: &'a mut Option<InputPageSession>,
    pub help_visible: &'a mut bool,
    pub notice: &'a mut NoticeState,
    pub mouse_selection: &'a mut MouseSelection,
    pub pane_resize: &'a mut PaneResizeState,
    pub approval: &'a mut Option<ApprovalCard>,
    pub question: &'a mut Option<String>,
    pub queue: &'a mut PendingPromptQueue,
    pub config: &'a mut Config,
    pub themes: &'a mut Vec<ThemeFile>,
    pub theme: &'a mut Theme,
}

pub struct RuntimeController;

fn normalized_session_status(status: &crate::agent::AgentStatus) -> crate::SessionStatus {
    match status {
        crate::agent::AgentStatus::Running | crate::agent::AgentStatus::Waiting => {
            crate::SessionStatus::Running
        }
        crate::agent::AgentStatus::Idle
        | crate::agent::AgentStatus::Error
        | crate::agent::AgentStatus::Custom(_) => crate::SessionStatus::Idle,
    }
}

impl RuntimeController {
    pub fn apply_terminal_route(
        route: TerminalRoute,
        size: TerminalSize,
        now: Instant,
        state: &Arc<Mutex<RuntimeState>>,
        selection_frame: &SelectionFrame,
        ui: &mut TerminalUiState<'_>,
    ) -> Vec<UiAction> {
        terminal::apply_terminal_route(route, size, now, state, selection_frame, ui)
    }

    /// Apply one normalized agent fact and return deferred external effects.
    pub fn apply_agent(
        event: AgentEvent,
        state: &Arc<Mutex<RuntimeState>>,
        ui: &mut RuntimeUiState<'_>,
    ) -> Vec<UiAction> {
        agent::apply_agent(event, state, ui)
    }

    pub fn apply_action(action: ControllerAction, input_page: &mut Option<InputPageSession>) {
        input::apply_action(action, input_page)
    }

    pub fn apply_input_action(
        action: InputAction,
        state: &Mutex<RuntimeState>,
        queue: &mut PendingPromptQueue,
    ) -> InputHandlerOutcome {
        input::apply_input_action(action, state, queue)
    }

    pub fn answer_approval(key: &KeyEvent, approval: &mut Option<ApprovalCard>) -> Vec<UiAction> {
        input::answer_approval(key, approval)
    }

    pub fn apply_input_page_key(
        key: &KeyEvent,
        state: &Mutex<RuntimeState>,
        ui: &mut InputPageUiState<'_>,
    ) -> Vec<UiAction> {
        input::apply_input_page_key(key, state, ui)
    }

    pub fn apply_reloaded_config(
        config: Config,
        themes: Vec<ThemeFile>,
        state: &Mutex<RuntimeState>,
        ui: &mut TerminalUiState<'_>,
    ) {
        effect::apply_reloaded_config(config, themes, state, ui)
    }

    pub fn apply_effect_result(
        result: EffectResult,
        state: &Mutex<RuntimeState>,
        now: Instant,
    ) -> bool {
        effect::apply_effect_result(result, state, now)
    }

    pub fn dispatch_next_queued(state: &Mutex<RuntimeState>) -> Vec<UiAction> {
        effect::dispatch_next_queued(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::InteractionEvent,
        display::DisplayId,
        input::InputState,
        preview::{PreviewContent, PreviewLayoutKey, PreviewRef, PreviewTarget},
        render::RenderOptions,
        reveal::LineRevealTrack,
        Language,
    };
    use ratatui::text::Line;

    fn open_help_suggestion(input: &mut InputState, catalogs: &crate::CatalogModel) {
        input.handle_key_with_catalog(
            &KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            true,
            catalogs,
        );
        let suggestion = input.suggest.as_mut().expect("slash opens suggestions");
        let selected = suggestion
            .matches
            .iter()
            .position(|line| line == "/help")
            .expect("help is a built-in command");
        suggestion.sel = selected;
    }

    fn seed_localized_presentation(
        state: &mut RuntimeState,
    ) -> (DisplayId, PreviewLayoutKey, usize) {
        let id = DisplayId::correlated("controller", "markdown");
        let theme = state.config.theme();
        let options = RenderOptions {
            language: state.config.language,
            content_width: Some(80),
            ..Default::default()
        };
        let source = "```rust\nx\n```";
        let render = &mut state.render;
        render.markdown_layout.materialize(
            &id,
            source,
            &theme,
            &mut render.next_unit,
            &options,
            &mut render.units,
        );
        render.transcript_cache.valid = true;

        let target = PreviewTarget {
            id: "preview-owner".into(),
            reference: PreviewRef::Inline {
                key: crate::PreviewKey("preview-owner".into()),
                revision: crate::PreviewRevision(1),
                content: PreviewContent::PlainText("semantic preview".into()),
            },
        };
        state.preview.select(Some(target));
        let mut reveal = LineRevealTrack::default();
        reveal.reconcile(
            &[Line::from("first"), Line::from("second")],
            Instant::now(),
            32,
        );
        let revealed = reveal.revealed();
        state.preview.reveal = Some(reveal);
        let layout_key = PreviewLayoutKey {
            owner: Some((
                crate::PreviewKey("preview-owner".into()),
                crate::PreviewRevision(1),
            )),
            content_signature: 0,
            width: 40,
            theme_signature: 7,
        };
        state
            .preview
            .store_layout(layout_key.clone(), vec![Line::from("cached")]);
        (id, layout_key, revealed)
    }

    fn terminal_ui<'a>(
        scroll: &'a mut ScrollState,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
        help_visible: &'a mut bool,
        notice: &'a mut NoticeState,
        mouse_selection: &'a mut MouseSelection,
        pane_resize: &'a mut PaneResizeState,
        approval: &'a mut Option<ApprovalCard>,
        question: &'a mut Option<String>,
        queue: &'a mut PendingPromptQueue,
        config: &'a mut Config,
        themes: &'a mut Vec<ThemeFile>,
        theme: &'a mut Theme,
    ) -> TerminalUiState<'a> {
        TerminalUiState {
            scroll,
            input,
            input_page,
            help_visible,
            notice,
            mouse_selection,
            pane_resize,
            approval,
            question,
            queue,
            config,
            themes,
            theme,
        }
    }

    fn runtime_ui<'a>(
        scroll: &'a mut ScrollState,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
        approval: &'a mut Option<ApprovalCard>,
        question: &'a mut Option<String>,
        queue: &'a mut PendingPromptQueue,
    ) -> RuntimeUiState<'a> {
        RuntimeUiState {
            scroll,
            input,
            input_page,
            approval,
            question,
            queue,
        }
    }

    #[test]
    fn settings_language_change_updates_live_state_and_invalidates_localized_caches() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut config = Config::default();
        let (markdown_id, layout_key, revealed) = {
            let mut app = state.lock().unwrap();
            seed_localized_presentation(&mut app)
        };
        let catalogs = state.lock().unwrap().catalogs.clone();
        let mut input = InputState::new(&config);
        open_help_suggestion(&mut input, &catalogs);

        let mut page = Some(InputPageSession::settings(crate::settings::SettingsState {
            category: 1,
            pos: [0, 1, 0, 0],
            ..Default::default()
        }));
        page.as_mut().unwrap().rebuild_focus();
        assert_eq!(
            page.as_ref()
                .unwrap()
                .focus
                .current
                .as_ref()
                .map(|id| id.0.as_str()),
            Some("settings:item:1:language")
        );
        let themes = Vec::new();
        let mut theme = config.theme();
        let mut question = None;

        let mut apply_key = |key| {
            RuntimeController::apply_input_page_key(
                &key,
                &state,
                &mut InputPageUiState {
                    input_page: &mut page,
                    input: &mut input,
                    config: &mut config,
                    themes: &themes,
                    theme: &mut theme,
                    question: &mut question,
                },
            )
        };
        assert!(apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).is_empty());
        assert!(apply_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)).is_empty());
        let effects = apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        drop(apply_key);

        assert_eq!(config.language, Language::SimplifiedChinese);
        assert_eq!(
            state.lock().unwrap().config.language,
            Language::SimplifiedChinese
        );
        assert_eq!(input.language, Language::SimplifiedChinese);
        assert!(
            matches!(effects.as_slice(), [UiAction::PersistConfig(snapshot)] if snapshot.language == Language::SimplifiedChinese)
        );
        assert_eq!(
            page.as_ref()
                .unwrap()
                .focus
                .current
                .as_ref()
                .map(|id| id.0.as_str()),
            Some("settings:item:1:language")
        );

        let mut app = state.lock().unwrap();
        assert!(!app.render.transcript_cache.valid);
        assert!(app.render.markdown_layout.lines(&markdown_id).is_some());
        assert!(app.preview.cached_layout(&layout_key).is_none());
        assert_eq!(app.preview.target.as_ref().unwrap().id, "preview-owner");
        assert!(app
            .preview
            .cache
            .get(
                &crate::PreviewKey("preview-owner".into()),
                crate::PreviewRevision(1),
            )
            .is_some());
        assert_eq!(
            app.preview.reveal.as_ref().map(LineRevealTrack::revealed),
            Some(revealed)
        );
        let options = RenderOptions {
            language: app.config.language,
            content_width: Some(80),
            ..Default::default()
        };
        let theme = app.config.theme();
        let render = &mut app.render;
        let lines = render
            .markdown_layout
            .materialize(
                &markdown_id,
                "```rust\nx\n```",
                &theme,
                &mut render.next_unit,
                &options,
                &mut render.units,
            )
            .to_vec();
        let text = lines[0]
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(
            text.contains("行"),
            "localized Markdown cache was rebuilt: {text:?}"
        );
        assert_eq!(input.suggest.as_ref().unwrap().query, "/");
        let suggestion = input.suggest.as_ref().unwrap();
        let selected = suggestion
            .matches
            .iter()
            .position(|line| line == "/help")
            .unwrap();
        assert_eq!(suggestion.sel, selected);
        assert_eq!(suggestion.descriptions[selected], "显示帮助");
    }

    #[test]
    fn reload_language_refreshes_suggestions_without_resetting_preview_semantics() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut old_config = Config::default();
        let (markdown_id, layout_key, revealed) = {
            let mut app = state.lock().unwrap();
            seed_localized_presentation(&mut app)
        };
        let catalogs = state.lock().unwrap().catalogs.clone();
        let mut input = InputState::new(&old_config);
        open_help_suggestion(&mut input, &catalogs);
        let selected_before = input.suggest.as_ref().unwrap().sel;

        let mut new_config = old_config.clone();
        new_config.language = Language::SimplifiedChinese;
        let mut page = None;
        let mut help_visible = false;
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut pane_resize = PaneResizeState::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = PendingPromptQueue::default();
        let mut themes = Vec::new();
        let mut theme = old_config.theme();
        RuntimeController::apply_reloaded_config(
            new_config,
            Vec::new(),
            &state,
            &mut terminal_ui(
                &mut ScrollState::default(),
                &mut input,
                &mut page,
                &mut help_visible,
                &mut notice,
                &mut mouse_selection,
                &mut pane_resize,
                &mut approval,
                &mut question,
                &mut queue,
                &mut old_config,
                &mut themes,
                &mut theme,
            ),
        );

        assert_eq!(old_config.language, Language::SimplifiedChinese);
        assert_eq!(input.language, Language::SimplifiedChinese);
        let suggestion = input.suggest.as_ref().unwrap();
        assert_eq!(suggestion.query, "/");
        assert_eq!(suggestion.sel, selected_before);
        assert_eq!(suggestion.descriptions[suggestion.sel], "显示帮助");
        let app = state.lock().unwrap();
        assert!(!app.render.transcript_cache.valid);
        assert!(app.render.markdown_layout.lines(&markdown_id).is_some());
        assert!(app.preview.cached_layout(&layout_key).is_none());
        assert_eq!(app.preview.target.as_ref().unwrap().id, "preview-owner");
        assert_eq!(
            app.preview.reveal.as_ref().map(LineRevealTrack::revealed),
            Some(revealed)
        );
    }

    #[test]
    fn external_editor_text_only_targets_the_visible_composer() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = PendingPromptQueue::default();
        let effects = RuntimeController::apply_agent(
            AgentEvent::Interaction(InteractionEvent::SetEditorText {
                text: "visible draft".into(),
            }),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.buf, "visible draft");
        assert!(matches!(
            effects.as_slice(),
            [UiAction::RequestDraw(DrawPriority::Interactive)]
        ));

        page = Some(InputPageSession::login());
        RuntimeController::apply_agent(
            AgentEvent::Interaction(InteractionEvent::SetEditorText {
                text: "hidden".into(),
            }),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.buf, "visible draft");
    }

    #[test]
    fn clipboard_image_completion_targets_only_the_visible_composer() {
        let state = Mutex::new(RuntimeState::default());
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        assert!(RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Ok(ClipboardPaste::Image(image.clone()))),
            &state,
            Instant::now(),
        ));
        assert_eq!(
            state.lock().unwrap().interaction.input.display_text().text,
            "[Image clip.png]"
        );

        state.lock().unwrap().interaction.input_page = Some(InputPageSession::login());
        assert!(!RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Ok(ClipboardPaste::Image(image))),
            &state,
            Instant::now(),
        ));
        assert_eq!(
            state.lock().unwrap().interaction.input.display_text().text,
            "[Image clip.png]"
        );
    }

    #[test]
    fn queued_prompt_is_claimed_atomically_when_idle() {
        let state = Mutex::new(RuntimeState::default());
        state
            .lock()
            .unwrap()
            .interaction
            .queue
            .push("next".into(), crate::interaction::PromptDelivery::AfterTurn);
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(state.lock().unwrap().interaction.queue.is_empty());
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt })] if prompt.plain_text() == Some("next")
        ));
    }

    #[test]
    fn image_prompt_queues_and_dispatches_without_losing_bytes() {
        let state = Mutex::new(RuntimeState::default());
        state.lock().unwrap().session.status = crate::SessionStatus::Running;
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        let prompt = PromptInput {
            parts: vec![crate::PromptPart::Image(image.clone())],
        };
        let mut queue = PendingPromptQueue::default();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send(prompt.clone()),
            &state,
            &mut queue,
        );
        assert!(outcome.effects.is_empty());
        assert_eq!(queue.entries()[0].prompt, prompt);
        assert_eq!(
            queue.entries()[0].delivery,
            crate::interaction::PromptDelivery::Asap
        );

        {
            let mut app = state.lock().unwrap();
            app.session.status = crate::SessionStatus::Idle;
            app.session.working = false;
            app.interaction.queue = queue;
        }
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt: sent })] if sent == &prompt
        ));
    }

    #[test]
    fn asap_dispatches_during_a_turn_while_after_turn_waits_for_idle() {
        let state = Mutex::new(RuntimeState::default());
        {
            let mut app = state.lock().unwrap();
            app.session.status = crate::SessionStatus::Running;
            app.session.working = true;
        }
        let mut queue = PendingPromptQueue::default();

        let asap = RuntimeController::apply_input_action(
            InputAction::Send("asap".into()),
            &state,
            &mut queue,
        );
        let after = RuntimeController::apply_input_action(
            InputAction::SendAfterTurn("after".into()),
            &state,
            &mut queue,
        );
        assert!(asap.effects.is_empty() && after.effects.is_empty());
        state.lock().unwrap().interaction.queue = queue;

        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Steer { prompt })] if prompt.plain_text() == Some("asap")
        ));
        assert!(RuntimeController::dispatch_next_queued(&state).is_empty());

        {
            let mut app = state.lock().unwrap();
            app.session.status = crate::SessionStatus::Idle;
            app.session.working = false;
        }
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt })] if prompt.plain_text() == Some("after")
        ));
    }

    #[test]
    fn enter_after_turn_end_dispatches_directly_once_status_settles() {
        let state = Mutex::new(RuntimeState::default());
        state.lock().unwrap().session.status = crate::SessionStatus::Running;
        let mut queue = PendingPromptQueue::default();

        let outcome = RuntimeController::apply_input_action(
            InputAction::Send("next turn".into()),
            &state,
            &mut queue,
        );
        assert!(outcome.effects.is_empty());
        state.lock().unwrap().interaction.queue = queue;

        assert!(RuntimeController::dispatch_next_queued(&state).is_empty());
        state.lock().unwrap().session.status = crate::SessionStatus::Idle;
        let effects = RuntimeController::dispatch_next_queued(&state);
        assert!(matches!(
            effects.as_slice(),
            [UiAction::Agent(AgentRequest::Input { prompt })]
                if prompt.plain_text() == Some("next turn")
        ));
    }

    #[test]
    fn escape_cancels_the_latest_candidate_before_interrupting() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().session.status = crate::SessionStatus::Running;
        let mut config = Config::default();
        let mut input = InputState::new(&config);
        let mut scroll = ScrollState::default();
        let mut page = None;
        let mut help_visible = false;
        let mut notice = NoticeState::default();
        let mut mouse_selection = MouseSelection::default();
        let mut pane_resize = PaneResizeState::default();
        let mut approval = None;
        let mut question = None;
        let mut queue = PendingPromptQueue::default();
        queue.push("a".into(), crate::interaction::PromptDelivery::Asap);
        queue.push("b".into(), crate::interaction::PromptDelivery::AfterTurn);
        let mut themes = Vec::new();
        let mut theme = config.theme();

        let effects = RuntimeController::apply_terminal_route(
            TerminalRoute::Ordinary(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TerminalSize {
                width: 80,
                height: 30,
            },
            Instant::now(),
            &state,
            &SelectionFrame::default(),
            &mut terminal_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut help_visible,
                &mut notice,
                &mut mouse_selection,
                &mut pane_resize,
                &mut approval,
                &mut question,
                &mut queue,
                &mut config,
                &mut themes,
                &mut theme,
            ),
        );
        assert!(
            effects.is_empty(),
            "candidate cancellation must not interrupt"
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.entries()[0].prompt, "a");
    }

    #[test]
    fn failed_image_draft_materialization_restores_the_atomic_block() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let image = PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some("clip.png".into()),
        };
        let prompt = PromptInput {
            parts: vec![crate::PromptPart::Image(image)],
        };
        let mut queue = PendingPromptQueue::default();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send(prompt.clone()),
            state.as_ref(),
            &mut queue,
        );
        assert!(matches!(
            outcome.effects.as_slice(),
            [UiAction::Agent(AgentRequest::NewInput { prompt: sent, .. })] if sent == &prompt
        ));

        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        RuntimeController::apply_agent(
            AgentEvent::Session(crate::agent::SessionEvent::Attached(
                crate::agent::AttachedSession {
                    protocol_version: Some(7),
                    max_frame_bytes: None,
                    id: "new-session".into(),
                    status: crate::agent::AgentStatus::Idle,
                    provider: None,
                    model: None,
                    mode: Some("standard".into()),
                    title: None,
                    workspace: None,
                },
            )),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(state.lock().unwrap().is_new_conversation());

        agent::apply_agent_error(
            "new-input-failed",
            "bad image",
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert_eq!(input.display_text().text, "[Image clip.png]");
    }

    #[test]
    fn first_direct_user_message_commits_the_attached_new_conversation() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        state.lock().unwrap().begin_new_conversation("standard");
        let _ = state
            .lock()
            .unwrap()
            .materialize_new_conversation(PromptInput::text("first"));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = PendingPromptQueue::default();
        RuntimeController::apply_agent(
            AgentEvent::Timeline(crate::agent::TimelineEvent::Append(
                crate::agent::TimelineRecord {
                    sequence: Some(1),
                    time_ms: None,
                    surface: Some(crate::agent::SurfaceOperation::Append),
                    source_sequences: Vec::new(),
                    fact: crate::agent::TimelineFact::UserMessage {
                        text: "first".into(),
                        source_kind: Some("user".into()),
                        content: vec![crate::agent::ContentBlock::Text("first".into())],
                        source: Default::default(),
                    },
                },
            )),
            &state,
            &mut runtime_ui(
                &mut scroll,
                &mut input,
                &mut page,
                &mut approval,
                &mut question,
                &mut queue,
            ),
        );
        assert!(!state.lock().unwrap().is_new_conversation());
    }

    #[test]
    fn clipboard_read_failure_becomes_visible_without_reentrant_locking() {
        let state = Mutex::new(RuntimeState::default());
        assert!(RuntimeController::apply_effect_result(
            EffectResult::ClipboardRead(Err("denied".into())),
            &state,
            Instant::now(),
        ));
        assert!(!state.lock().unwrap().transcript.is_empty());
    }
}
