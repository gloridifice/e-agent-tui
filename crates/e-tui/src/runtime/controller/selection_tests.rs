use super::*;
use crate::{
    display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat},
    input::{Suggestion, SuggestionKind},
    input_page::InputPage,
    runtime::{
        execute_ui_actions, DirtyReason, FrameScheduler, ScriptedAgentRequestPort,
        ScriptedUiActionPorts,
    },
    ui::{Presentation, RenderOverlays},
};
use ratatui::{
    backend::TestBackend,
    buffer::{Buffer, CellWidth},
    Terminal,
};
use std::time::Duration;

struct Harness {
    state: Arc<Mutex<RuntimeState>>,
    terminal: Terminal<TestBackend>,
    committed: Presentation,
    scheduler: FrameScheduler,
    now: Instant,
}

impl Harness {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(RuntimeState::default())),
            terminal: Terminal::new(TestBackend::new(120, 40)).unwrap(),
            committed: Presentation::default(),
            scheduler: FrameScheduler::new(now),
            now,
        }
    }

    fn reconcile(&mut self) {
        RuntimeController::reconcile_presentation(
            &self.state,
            &self.committed,
            &mut self.scheduler,
            self.now,
        );
    }

    fn render(&mut self) {
        self.reconcile();
        self.scheduler.take_due(self.now + Duration::from_secs(1));
        let mut state = self.state.lock().unwrap();
        let mut i = std::mem::take(&mut state.interaction);
        let theme = state.theme();
        let mut candidate = None;
        let toast = i
            .notice
            .visible_text(state.config.copy_toast_secs, self.now);
        self.terminal
            .draw(|frame| {
                candidate = Some(
                    crate::ui::render_with_cursor_and_selection(
                        frame,
                        &mut state,
                        &i.input,
                        &mut i.scroll,
                        &theme,
                        RenderOverlays {
                            help_visible: i.help_visible,
                            toast,
                            input_page: i.input_page.as_mut(),
                            settings: None,
                            login: None,
                            approval: i.approval.as_ref(),
                            queue: i.queue.entries(),
                            pane_resize: i.pane_resize,
                        },
                        &i.mouse_selection,
                        &self.committed,
                    )
                    .presentation,
                );
            })
            .unwrap();
        self.committed
            .commit(candidate.unwrap(), &mut i.mouse_selection);
        state.interaction = i;
        self.scheduler.complete(self.now);
    }

    fn route(&mut self, route: TerminalRoute) -> Vec<UiAction> {
        let (mut i, mut config) = {
            let mut state = self.state.lock().unwrap();
            (std::mem::take(&mut state.interaction), state.config.clone())
        };
        let mut theme = config.theme();
        let size = self.terminal.size().unwrap();
        let effects = RuntimeController::apply_terminal_route(
            route,
            TerminalSize {
                width: size.width,
                height: size.height,
            },
            self.now,
            &self.state,
            self.committed.selection_frame(),
            &mut TerminalUiState {
                scroll: &mut i.scroll,
                input: &mut i.input,
                input_page: &mut i.input_page,
                help_visible: &mut i.help_visible,
                notice: &mut i.notice,
                mouse_selection: &mut i.mouse_selection,
                pane_resize: &mut i.pane_resize,
                approval: &mut i.approval,
                question: &mut i.question,
                queue: &mut i.queue,
                config: &mut config,
                themes: &mut Vec::new(),
                theme: &mut theme,
            },
        );
        self.state.lock().unwrap().interaction = i;
        self.scheduler.request(DirtyReason::Interactive, self.now);
        self.reconcile();
        effects
    }

    fn pointer(&mut self, pointer: PointerEvent) -> Vec<UiAction> {
        self.route(TerminalRoute::Pointer(pointer))
    }

    fn press(&mut self, at: (u16, u16)) {
        assert!(self
            .pointer(PointerEvent::PrimaryPress {
                column: at.0,
                row: at.1
            })
            .is_empty());
    }

    fn release(&mut self, at: (u16, u16)) -> Vec<UiAction> {
        self.pointer(PointerEvent::PrimaryRelease {
            column: at.0,
            row: at.1,
        })
    }

    fn locate(&self, text: &str) -> (u16, u16) {
        locate(self.terminal.backend().buffer(), text)
            .unwrap_or_else(|| panic!("missing {text:?}: {:?}", self.terminal.backend().buffer()))
    }

    fn copy_text(&mut self, text: &str) {
        let (x, y) = self.locate(text);
        self.press((x, y));
        let effects = self.release((x + text.cell_width().max(2) - 1, y));
        assert!(
            matches!(effects.as_slice(), [UiAction::WriteClipboard(payload)] if payload == text),
            "wrong copy: {effects:?}"
        );
    }

    fn copy_screen(&mut self) -> String {
        self.press((0, 0));
        match self.release((119, 39)).pop().unwrap() {
            UiAction::WriteClipboard(text) => text,
            other => panic!("unexpected {other:?}"),
        }
    }
}

fn locate(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let mut tail = String::new();
            let mut column = x;
            while column < buffer.area.width {
                let cell = &buffer[(column, y)];
                tail.push_str(cell.symbol());
                column += cell.cell_width().max(1);
            }
            if tail.starts_with(needle) {
                return Some((x, y));
            }
        }
    }
    None
}

#[tokio::test]
async fn inbound_link_validation_completion_renders_and_copies_the_tagged_url() {
    use crate::agent::{TimelineEvent, TimelineFact, TimelineRecord};
    let mut h = Harness::new();
    h.state.lock().unwrap().config.message_chars_per_second =
        crate::config::RevealRate::new(0).unwrap();
    let mut interaction = std::mem::take(&mut h.state.lock().unwrap().interaction);
    let actions = RuntimeController::apply_agent(
        AgentEvent::Timeline(TimelineEvent::Append(TimelineRecord {
            sequence: Some(1),
            time_ms: None,
            surface: None,
            source_sequences: Vec::new(),
            fact: TimelineFact::AssistantMessage {
                text: "请检查 [Rust 文档](https://www.rust-lang.org/learn)。".into(),
                reasoning: String::new(),
                content: Vec::new(),
                turn: Some(1),
                step: Some(1),
                usage: None,
            },
        })),
        &h.state,
        &mut RuntimeUiState {
            scroll: &mut interaction.scroll,
            input: &mut interaction.input,
            input_page: &mut interaction.input_page,
            approval: &mut interaction.approval,
            question: &mut interaction.question,
            queue: &mut interaction.queue,
        },
    );
    h.state.lock().unwrap().interaction = interaction;
    let mut agent = ScriptedAgentRequestPort::default();
    let mut ports = ScriptedUiActionPorts::successful(h.now);
    let execution = execute_ui_actions(actions, &mut agent, &mut h.scheduler, &mut ports).await;
    assert!(execution
        .completed
        .iter()
        .any(|result| matches!(result, EffectResult::LinksValidated { .. })));
    for result in execution.completed {
        RuntimeController::apply_effect_result(result, &h.state, h.now);
    }
    h.render();
    h.locate("https://www.rust-lang.org/learn~1");
    h.route(TerminalRoute::Global(crate::key_mapping::Action::CopyLink));
    let copy = h.route(TerminalRoute::Ordinary(KeyEvent::new(
        KeyCode::Char('1'),
        KeyModifiers::NONE,
    )));
    execute_ui_actions(copy, &mut agent, &mut h.scheduler, &mut ports).await;
    assert_eq!(ports.clipboard_writes, ["https://www.rust-lang.org/learn"]);
}

#[test]
fn selection_copies_composer_accessories_status_path_and_rules() {
    let mut h = Harness::new();
    {
        let mut state = h.state.lock().unwrap();
        state.interaction.input.paste("draft 界🙂 text");
        state.session.session_title = Some("A session title".into());
        state.session.session_cwd = Some("D:/work/project".into());
        state.session.model = Some("selected-model".into());
        state.goal = Some("goal accessory".into());
    }
    h.render();
    for text in [
        "draft 界🙂 text",
        "A session title",
        "D:/work/project",
        "selected-model",
        "goal accessory",
    ] {
        h.copy_text(text);
    }
    let before = h.state.lock().unwrap().interaction.input.buf.clone();
    let screen = h.copy_screen();
    assert!(screen.contains('─'));
    assert_eq!(h.state.lock().unwrap().interaction.input.buf, before);
}

#[test]
fn selection_copies_page_masks_and_collapsed_labels_not_source() {
    let mut h = Harness::new();
    {
        let mut state = h.state.lock().unwrap();
        state.interaction.input.paste_placeholder_chars = 5;
        state.interaction.input.paste("hidden paste contents");
        state.interaction.input.paste_image(PromptImage {
            name: Some("photo.png".into()),
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
        });
    }
    h.render();
    let text = h.copy_screen();
    assert!(text.contains("text pasted"));
    assert!(text.contains("photo.png"));
    assert!(!text.contains("hidden paste contents"));
    assert!(!text.contains('\u{fffc}'));
    {
        let mut state = h.state.lock().unwrap();
        let mut page = InputPageSession::login();
        if let InputPage::Login(login) = &mut page.page {
            login.loading = false;
            login.editing = Some("secret-key-value".into());
            login.page = crate::login::Page::ApiKey {
                provider: "provider-id".into(),
                buf: "secret-key-value".into(),
            };
        }
        state.interaction.input_page = Some(page);
    }
    h.render();
    let copied = h.copy_screen();
    assert!(copied.contains("/login"));
    assert!(copied.contains("●●"));
    assert!(!copied.contains("secret-key-value"));
    assert!(!copied.contains("photo.png"));
    h.state.lock().unwrap().interaction.input_page =
        Some(InputPageSession::settings(Default::default()));
    h.render();
    h.copy_text("/settings");
}

#[test]
fn selection_copies_topmost_suggestions_help_toasts_and_visible_ellipsis() {
    let mut h = Harness::new();
    {
        let mut state = h.state.lock().unwrap();
        state.session.session_title = Some("verylongtitle".repeat(20));
        state.session.session_cwd = Some("D:/keep-path".into());
        state.interaction.input.suggest = Some(Suggestion {
            query: "/s".into(),
            sel: 0,
            matches: vec!["/settings".into()],
            descriptions: vec!["suggestion description".into()],
            sources: vec![crate::command_catalog::CommandSource::Builtin],
            kind: SuggestionKind::Commands,
        });
        state.interaction.notice.show("selected notice", h.now);
    }
    h.render();
    h.copy_text("suggestion description");
    h.copy_text("selected notice");
    let copied = h.copy_screen();
    assert!(copied.contains('…'));
    assert!(!copied.contains(&"verylongtitle".repeat(20)));
    h.state.lock().unwrap().interaction.help_visible = true;
    h.render();
    h.copy_text("Help");
}

#[test]
fn selection_holds_screen_without_materialization_and_restores_deferred_updates() {
    let mut h = Harness::new();
    {
        let mut state = h.state.lock().unwrap();
        state.session.session_title = Some("old title".into());
        state.interaction.notice.show("held notice", h.now);
    }
    h.render();
    let at = h.locate("held notice");
    h.press(at);
    {
        let mut state = h.state.lock().unwrap();
        state.session.session_title = Some("new title".into());
        state.push_system_message("background update");
        state.tick_reveals(h.now);
        state.render.transcript_cache.take_work_stats();
        state.preview.take_work_stats();
    }
    h.now += Duration::from_secs(10);
    h.scheduler.request(DirtyReason::Content, h.now);
    h.pointer(PointerEvent::PrimaryDrag {
        column: at.0 + 10,
        row: at.1,
    });
    h.render();
    assert!(locate(h.terminal.backend().buffer(), "new title").is_none());
    h.locate("old title");
    h.locate("held notice");
    {
        let mut state = h.state.lock().unwrap();
        let work = state.render.transcript_cache.take_work_stats();
        let preview = state.preview.take_work_stats();
        assert_eq!(
            (work.rebuilds, work.patches, work.materialized_rows),
            (0, 0, 0)
        );
        assert_eq!(
            (preview.rebuilds, preview.patches, preview.materialized_rows),
            (0, 0, 0)
        );
        assert!(h
            .scheduler
            .presentation_deadline(state.interaction.notice.deadline(3))
            .is_none());
    }
    assert!(
        matches!(h.release((at.0 + 10, at.1)).as_slice(), [UiAction::WriteClipboard(text)] if text == "held notice")
    );
    assert!(h.scheduler.deadline().is_some());
    h.render();
    h.locate("new title");
    h.locate("background update");
    assert!(locate(h.terminal.backend().buffer(), "held notice").is_none());
}

#[test]
fn selection_never_expands_unrevealed_markdown() {
    let mut h = Harness::new();
    {
        let mut state = h.state.lock().unwrap();
        let id = DisplayId::correlated("selection", "reveal");
        state.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: id.clone(),
                unit: None,
                content: "unrevealedsecret".into(),
                copy_source: "unrevealedsecret".into(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                streaming: true,
            }),
            None,
        );
        state
            .render
            .transcript_reveals
            .insert(id, crate::reveal::RevealTrack::default());
    }
    h.render();
    assert!(!h.copy_screen().contains("unrevealedsecret"));
    assert!(h
        .state
        .lock()
        .unwrap()
        .render
        .units
        .values()
        .any(|s| s == "unrevealedsecret"));
}

#[test]
fn selection_cancels_on_input_context_changes_and_separator_capture_stays_exclusive() {
    let mut h = Harness::new();
    h.state
        .lock()
        .unwrap()
        .interaction
        .input
        .paste("draft text");
    h.render();
    let at = h.locate("draft text");
    for route in [
        TerminalRoute::Pointer(PointerEvent::FocusLost),
        TerminalRoute::Pointer(PointerEvent::Wheel { up: true }),
        TerminalRoute::Paste {
            text: "paste".into(),
        },
        TerminalRoute::ReadClipboard,
        TerminalRoute::OpenHelp,
    ] {
        h.press(at);
        h.route(route);
        assert!(!h
            .state
            .lock()
            .unwrap()
            .interaction
            .mouse_selection
            .is_dragging());
        assert!(h.release((at.0 + 3, at.1)).is_empty());
        h.state.lock().unwrap().interaction.help_visible = false;
    }
    h.render();
    h.press((0, 0));
    h.state.lock().unwrap().session.session_id = Some("different-session".into());
    h.reconcile();
    assert!(h.release((5, 0)).is_empty());
    // The old screen is not an eligible anchor in a new, not-yet-painted context.
    h.press((0, 0));
    assert!(!h
        .state
        .lock()
        .unwrap()
        .interaction
        .mouse_selection
        .is_dragging());
    h.render();
    h.press((0, 0));
    h.state.lock().unwrap().interaction.approval = Some(ApprovalCard {
        id: "new-approval".into(),
        tool_name: "tool".into(),
        reason: "reason".into(),
    });
    h.reconcile();
    assert!(h.release((5, 0)).is_empty());
    h.render();
    let separator = h
        .state
        .lock()
        .unwrap()
        .config
        .message_pane_percent
        .columns(120);
    h.press((separator, 20));
    assert!(h.state.lock().unwrap().interaction.pane_resize.is_active());
    assert!(!h
        .state
        .lock()
        .unwrap()
        .interaction
        .mouse_selection
        .is_dragging());
    h.pointer(PointerEvent::PrimaryDrag { column: 50, row: 5 });
    assert!(matches!(
        h.release((50, 5)).as_slice(),
        [UiAction::PersistConfig(_)]
    ));
}

#[test]
fn selection_resize_cannot_leave_the_presentation_clock_held() {
    let mut h = Harness::new();
    h.render();
    h.press((0, 0));
    h.terminal.backend_mut().resize(100, 30);
    h.render();
    assert!(!h
        .state
        .lock()
        .unwrap()
        .interaction
        .mouse_selection
        .is_dragging());
    // Auto-resize can be observed during draw before its input report arrives.
    h.reconcile();
    assert!(h.scheduler.deadline().is_some());
    assert_eq!(h.scheduler.presentation_deadline(Some(h.now)), Some(h.now));
    assert!(h.release((5, 0)).is_empty());
}

#[tokio::test]
async fn selection_clipboard_effect_runs_unlocked_once_and_reports_both_outcomes() {
    for succeeds in [true, false] {
        let mut h = Harness::new();
        h.state.lock().unwrap().interaction.input.paste("copy this");
        h.render();
        let at = h.locate("copy this");
        h.press(at);
        let effects = h.release((at.0 + 8, at.1));
        assert!(h.state.try_lock().is_ok());
        let mut ports = ScriptedUiActionPorts::successful(h.now);
        if !succeeds {
            ports.clipboard_result = Err("clipboard denied".into());
        }
        let mut agent = ScriptedAgentRequestPort::default();
        let result = execute_ui_actions(effects, &mut agent, &mut h.scheduler, &mut ports).await;
        assert_eq!(ports.clipboard_writes, ["copy this"]);
        for result in result.completed {
            assert!(RuntimeController::apply_effect_result(
                result, &h.state, h.now
            ));
        }
        assert!(h.release((at.0 + 8, at.1)).is_empty());
        h.render();
        if succeeds {
            h.locate("Copied");
        } else {
            h.locate("clipboard denied");
        }
        assert_eq!(h.state.lock().unwrap().interaction.input.buf, "copy this");
    }
}
