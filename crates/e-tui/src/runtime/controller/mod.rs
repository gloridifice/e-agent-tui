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
    interaction::ApprovalCard,
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
    pub queue: &'a mut Vec<PromptInput>,
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
    pub queue: &'a mut Vec<PromptInput>,
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
        queue: &mut Vec<PromptInput>,
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
    use crate::{agent::InteractionEvent, input::InputState};

    fn runtime_ui<'a>(
        scroll: &'a mut ScrollState,
        input: &'a mut InputState,
        input_page: &'a mut Option<InputPageSession>,
        approval: &'a mut Option<ApprovalCard>,
        question: &'a mut Option<String>,
        queue: &'a mut Vec<PromptInput>,
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
    fn external_editor_text_only_targets_the_visible_composer() {
        let state = Arc::new(Mutex::new(RuntimeState::default()));
        let mut scroll = ScrollState::default();
        let mut input = InputState::new(&Config::default());
        let mut page = None;
        let mut approval = None;
        let mut question = None;
        let mut queue = Vec::new();
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
        state.lock().unwrap().interaction.queue.push("next".into());
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
        let mut queue = Vec::new();
        let outcome = RuntimeController::apply_input_action(
            InputAction::Send(prompt.clone()),
            &state,
            &mut queue,
        );
        assert!(outcome.effects.is_empty());
        assert_eq!(queue.as_slice(), std::slice::from_ref(&prompt));

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
        let mut queue = Vec::new();
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
        let mut queue = Vec::new();
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
