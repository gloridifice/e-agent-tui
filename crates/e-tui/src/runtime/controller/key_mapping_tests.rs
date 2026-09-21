use super::*;
use crate::runtime::input::{route_terminal_event_with_mapping, TerminalFocus};
use crate::Language;
use crate::{
    input_page::InputPage,
    key_mapping::{Action, KeyMapping, Platform, Scope},
    InteractionModel,
};

struct Harness {
    state: Arc<Mutex<RuntimeState>>,
    interaction: InteractionModel,
    config: Config,
    themes: Vec<ThemeFile>,
    theme: Theme,
}

impl Harness {
    fn new(source: &str) -> Self {
        let config = Config {
            key_mapping: KeyMapping::from_user_toml_for(source, Platform::Other).unwrap(),
            ..Config::default()
        };
        let mut state = RuntimeState::default();
        state.config.clone_from(&config);
        Self {
            state: Arc::new(Mutex::new(state)),
            interaction: InteractionModel::new(&config),
            theme: config.theme(),
            config,
            themes: Vec::new(),
        }
    }

    fn ui(&mut self) -> TerminalUiState<'_> {
        let i = &mut self.interaction;
        TerminalUiState {
            scroll: &mut i.scroll,
            input: &mut i.input,
            input_page: &mut i.input_page,
            help_visible: &mut i.help_visible,
            help_scroll: &mut i.help_scroll,
            notice: &mut i.notice,
            mouse_selection: &mut i.mouse_selection,
            pane_resize: &mut i.pane_resize,
            approval: &mut i.approval,
            question: &mut i.question,
            queue: &mut i.queue,
            config: &mut self.config,
            themes: &mut self.themes,
            theme: &mut self.theme,
        }
    }

    fn press(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Vec<UiAction> {
        let (reading_view_open, history_view_open) = {
            let state = self.state.lock().unwrap();
            (state.reading.is_some(), state.history_page.is_some())
        };
        let focus = TerminalFocus {
            help_visible: self.interaction.help_visible,
            input_page_open: self.interaction.input_page.is_some(),
            approval_open: self.interaction.approval.is_some(),
            reading_view_open,
            history_view_open,
        };
        let route = route_terminal_event_with_mapping(
            Event::Key(KeyEvent::new(code, modifiers)),
            focus,
            &self.config.key_mapping,
        );
        let state = self.state.clone();
        RuntimeController::apply_terminal_route(
            route,
            TerminalSize {
                width: 120,
                height: 40,
            },
            Instant::now(),
            &state,
            &SelectionFrame::default(),
            &mut self.ui(),
        )
    }

    fn reload(&mut self, config: Config) {
        let state = self.state.clone();
        RuntimeController::apply_reloaded_config(config, Vec::new(), &state, &mut self.ui());
    }

    fn agent_fact(&mut self, sequence: u64, fact: crate::agent::TimelineFact) -> Vec<UiAction> {
        let i = &mut self.interaction;
        RuntimeController::apply_agent(
            AgentEvent::Timeline(crate::agent::TimelineEvent::Append(
                crate::agent::TimelineRecord {
                    sequence: Some(sequence),
                    time_ms: None,
                    surface: None,
                    source_sequences: Vec::new(),
                    fact,
                },
            )),
            &self.state,
            &mut RuntimeUiState {
                scroll: &mut i.scroll,
                input: &mut i.input,
                input_page: &mut i.input_page,
                approval: &mut i.approval,
                question: &mut i.question,
                queue: &mut i.queue,
            },
        )
    }

    fn render(&mut self) {
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::render_with_cursor(
                    frame,
                    &mut self.state.lock().unwrap(),
                    &self.interaction.input,
                    &mut self.interaction.scroll,
                    &self.theme,
                    crate::ui::RenderOverlays {
                        help_visible: self.interaction.help_visible,
                        help_scroll: Some(&mut self.interaction.help_scroll),
                        toast: None,
                        input_page: None,
                        settings: None,
                        login: None,
                        approval: None,
                        queue: &[],
                        pane_resize: PaneResizeState::default(),
                    },
                );
            })
            .unwrap();
    }
}

#[test]
fn quick_links_key_mapping_copies_without_editing_and_respects_contexts() {
    for mapping in [
        "",
        "[global]\ncopy_link = 'f2'",
        "[global]\ncopy_link = 'nop'",
    ] {
        let mut h = Harness::new(mapping);
        h.interaction.input.restore_text("draft".into());
        h.state.lock().unwrap().link_copy.links = vec![crate::link_copy::TaggedLink {
            target: "https://example.com".into(),
            tag: '1',
        }];
        h.press(KeyCode::Char('y'), KeyModifiers::CONTROL);
        let actions = h.press(KeyCode::Char('1'), KeyModifiers::NONE);
        if mapping.is_empty() {
            assert!(
                matches!(actions.as_slice(), [UiAction::WriteClipboard(text)] if text == "https://example.com")
            );
            assert_eq!(h.interaction.input.buf, "draft");
            h.press(KeyCode::Char('y'), KeyModifiers::CONTROL);
            assert!(h.press(KeyCode::Esc, KeyModifiers::NONE).is_empty());
            assert_eq!(h.interaction.input.buf, "draft");
        } else {
            assert!(!actions
                .iter()
                .any(|action| matches!(action, UiAction::WriteClipboard(_))));
            assert_eq!(h.interaction.input.buf, "draft1");
        }
        if mapping.contains("f2") {
            h.press(KeyCode::F(2), KeyModifiers::NONE);
            assert!(matches!(
                h.press(KeyCode::Char('1'), KeyModifiers::NONE).as_slice(),
                [UiAction::WriteClipboard(_)]
            ));
        }
        h.interaction.input_page = Some(InputPageSession::model());
        h.press(KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert!(!h.state.lock().unwrap().link_copy.armed);
    }
}

#[test]
fn quick_links_agent_settlement_arms_targets_only_until_the_next_user_turn() {
    use crate::agent::TimelineFact;
    let mut h = Harness::new("");
    let actions = h.agent_fact(
        1,
        TimelineFact::AssistantChunk {
            text: "https://example.com".into(),
            reasoning: String::new(),
            turn: Some(1),
            step: Some(1),
            usage: None,
        },
    );
    assert!(!actions
        .iter()
        .any(|action| matches!(action, UiAction::ValidateLinks(_))));
    let actions = h.agent_fact(
        2,
        TimelineFact::AssistantMessage {
            text: "https://example.com".into(),
            reasoning: String::new(),
            content: Vec::new(),
            turn: Some(1),
            step: Some(1),
            usage: None,
        },
    );
    let request = actions
        .into_iter()
        .find_map(|action| match action {
            UiAction::ValidateLinks(request) => Some(request),
            _ => None,
        })
        .expect("settlement validates links");
    assert!(RuntimeController::apply_effect_result(
        EffectResult::LinksValidated {
            request: request.clone(),
            validations: vec![crate::link_copy::CandidateGroupValidation {
                alternatives: vec![crate::link_copy::PathValidation::NotRequired],
            }],
        },
        &h.state,
        Instant::now()
    ));
    assert_eq!(
        h.state.lock().unwrap().link_copy.target('1').as_deref(),
        Some("https://example.com")
    );
    h.agent_fact(
        3,
        TimelineFact::UserMessage {
            text: "next".into(),
            source_kind: Some("user".into()),
            content: Vec::new(),
            source: Default::default(),
        },
    );
    assert!(h.state.lock().unwrap().link_copy.links.is_empty());
    assert!(!RuntimeController::apply_effect_result(
        EffectResult::LinksValidated {
            request,
            validations: Vec::new()
        },
        &h.state,
        Instant::now()
    ));
}

#[test]
fn quick_links_latest_message_validation_and_source_layout_are_stale_safe() {
    use crate::display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat};
    let h = Harness::new("");
    let add = |app: &mut RuntimeState, seq, text: &str| {
        app.transcript.append(
            DisplayItem::Block(TranscriptBlock {
                id: DisplayId::correlated("assistant-answer", &format!("{seq}:1")),
                unit: None,
                content: text.into(),
                format: TranscriptFormat::Markdown,
                tone: DisplayTone::Normal,
                copy_source: text.into(),
                streaming: false,
            }),
            None,
        );
        app.refresh_link_copy();
        let actions = app.take_actions();
        let [UiAction::ValidateLinks(request)] = actions.as_slice() else {
            panic!("validation requested")
        };
        request.clone()
    };
    let old = add(&mut h.state.lock().unwrap(), 1, "https://old.example");
    let current = add(
        &mut h.state.lock().unwrap(),
        2,
        "[new](https://new.example)",
    );
    assert!(!RuntimeController::apply_effect_result(
        EffectResult::LinksValidated {
            request: old,
            validations: Vec::new()
        },
        &h.state,
        Instant::now()
    ));
    assert!(RuntimeController::apply_effect_result(
        EffectResult::LinksValidated {
            request: current.clone(),
            validations: vec![crate::link_copy::CandidateGroupValidation {
                alternatives: vec![crate::link_copy::PathValidation::NotRequired],
            }],
        },
        &h.state,
        Instant::now()
    ));
    {
        let mut app = h.state.lock().unwrap();
        crate::presentation::materialize_transcript(&mut app);
        let rows = app
            .render
            .markdown_layout
            .lines(&DisplayId::correlated("assistant-answer", "2:1"))
            .unwrap();
        assert!(rows
            .iter()
            .flat_map(|row| row.line.spans.iter())
            .any(|span| span.content == "~1"));
        assert!(app
            .render
            .units
            .values()
            .any(|source| source == "[new](https://new.example)"));
        assert!(!app
            .render
            .markdown_layout
            .lines(&DisplayId::correlated("assistant-answer", "1:1"))
            .unwrap()
            .iter()
            .flat_map(|row| &row.line.spans)
            .any(|span| span.content.contains("~1")));
        app.push_local_markdown("local https://help.example");
        app.refresh_link_copy();
        assert_eq!(
            app.link_copy.target('1').as_deref(),
            Some("https://new.example")
        );
        assert!(app.take_actions().is_empty());
        app.begin_new_conversation("default");
        assert!(app.link_copy.links.is_empty());
    }
    assert!(!RuntimeController::apply_effect_result(
        EffectResult::LinksValidated {
            request: current,
            validations: Vec::new()
        },
        &h.state,
        Instant::now()
    ));
}

#[test]
fn slash_copy_writes_the_latest_completed_assistant_markdown_source() {
    use crate::display::{DisplayId, DisplayItem, DisplayTone, TranscriptBlock, TranscriptFormat};

    let mut h = Harness::new("");
    {
        let mut app = h.state.lock().unwrap();
        for (correlation, content, source, streaming) in [
            ("1:1", "rendered first", "# first", false),
            ("2:1", "rendered second", "**second**", false),
            ("3:1", "partial third", "_partial_", true),
        ] {
            app.transcript.append(
                DisplayItem::Block(TranscriptBlock {
                    id: DisplayId::correlated("assistant-answer", correlation),
                    unit: None,
                    content: content.into(),
                    format: TranscriptFormat::Markdown,
                    tone: DisplayTone::Normal,
                    copy_source: source.into(),
                    streaming,
                }),
                None,
            );
        }
        app.push_local_markdown("# local notice");
    }
    h.agent_fact(
        4,
        crate::agent::TimelineFact::UserMessage {
            text: "newer user card".into(),
            source_kind: Some("user".into()),
            content: Vec::new(),
            source: Default::default(),
        },
    );

    for key in "/copy".chars() {
        assert!(h.press(KeyCode::Char(key), KeyModifiers::NONE).is_empty());
    }
    assert!(
        matches!(h.press(KeyCode::Enter, KeyModifiers::NONE).as_slice(), [UiAction::WriteClipboard(text)] if text == "**second**")
    );
}

#[test]
fn history_opens_top50_directly_and_tab_switches_without_query() {
    use crate::{
        execution_history::{HistoryQueryKind, HistoryQueryResult},
        history_page::HistoryView,
    };

    for command in ["/history", "/history show"] {
        let mut h = Harness::new("");
        {
            let mut app = h.state.lock().unwrap();
            app.session.session_id = Some("session".into());
            app.session.session_cwd = Some("root".into());
        }
        h.interaction.input.restore_text(command.into());
        let actions = h.press(KeyCode::Enter, KeyModifiers::NONE);
        let requests: Vec<_> = actions
            .iter()
            .filter_map(|action| match action {
                UiAction::QueryHistory(request) => Some(request.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(requests.len(), 1);
        let request = requests.into_iter().next().unwrap();
        assert_eq!(request.kind, HistoryQueryKind::Longest50);
        assert_eq!(request.after_offset, 0);
        assert_eq!(request.watermark, None);
        assert!(h.state.lock().unwrap().history_page.is_some());
        h.interaction.input.restore_text("preserved draft".into());
        let toggle = h.press(KeyCode::Tab, KeyModifiers::NONE);
        assert!(matches!(toggle.as_slice(), [UiAction::RequestDraw(_)]));
        assert_eq!(
            h.state.lock().unwrap().history_page.as_ref().unwrap().view,
            HistoryView::Timeline
        );
        assert!(RuntimeController::apply_effect_result(
            EffectResult::HistoryQueried {
                request: request.clone(),
                result: Ok(HistoryQueryResult {
                    path: "trace".into(),
                    records: Vec::new(),
                    ranked_calls: Vec::new(),
                    warnings: vec!["capture incomplete".into()],
                    watermark: 7,
                    next_offset: 7,
                    has_more: false,
                }),
            },
            &h.state,
            Instant::now(),
        ));
        assert_eq!(
            h.state.lock().unwrap().history_page.as_ref().unwrap().state,
            crate::history_page::HistoryLoadState::Empty
        );
        assert_eq!(
            h.state.lock().unwrap().history_page.as_ref().unwrap().view,
            HistoryView::Timeline
        );
        let toggle = h.press(KeyCode::Tab, KeyModifiers::NONE);
        assert!(matches!(toggle.as_slice(), [UiAction::RequestDraw(_)]));
        assert_eq!(
            h.state.lock().unwrap().history_page.as_ref().unwrap().view,
            HistoryView::Ranking
        );
        let actions = h.press(KeyCode::PageDown, KeyModifiers::NONE);
        assert!(actions
            .iter()
            .all(|action| matches!(action, UiAction::RequestDraw(_))));
        h.press(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(h.state.lock().unwrap().history_page.is_none());
        assert_eq!(h.interaction.input.buf, "preserved draft");
        assert!(!RuntimeController::apply_effect_result(
            EffectResult::HistoryQueried {
                request,
                result: Err("late error".into())
            },
            &h.state,
            Instant::now(),
        ));
    }
}

#[test]
fn key_mapping_global_pages_preserve_drafts_and_protect_modals() {
    let mut h = Harness::new("[global]\nchoose_model='f1'");
    h.interaction.input.paste("draft\ntext");
    let draft = h.interaction.input.buf.clone();
    assert!(h
        .press(KeyCode::Char('l'), KeyModifiers::CONTROL)
        .is_empty());
    assert!(h.interaction.input_page.is_none());
    assert!(matches!(
        h.press(KeyCode::F(1), KeyModifiers::NONE).as_slice(),
        [UiAction::Agent(AgentRequest::ModelGet)]
    ));
    assert!(matches!(
        h.interaction.input_page.as_ref().unwrap().page,
        InputPage::Model(_)
    ));
    assert!(h
        .press(KeyCode::Char('e'), KeyModifiers::CONTROL)
        .is_empty());
    assert!(matches!(
        h.interaction.input_page.as_ref().unwrap().page,
        InputPage::Model(_)
    ));
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(h.interaction.input.buf, draft);
    h.press(KeyCode::Char('e'), KeyModifiers::CONTROL);
    assert!(matches!(
        h.interaction.input_page.as_ref().unwrap().page,
        InputPage::Effort(_)
    ));
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    h.press(KeyCode::Char(','), KeyModifiers::CONTROL);
    assert!(matches!(
        h.interaction.input_page.as_ref().unwrap().page,
        InputPage::Settings(_)
    ));
}

#[test]
fn key_mapping_model_marks_persist_and_select_without_touching_the_draft() {
    let mut h = Harness::new("");
    h.interaction.input.paste("preserved\ndraft");
    let draft = h.interaction.input.buf.clone();
    h.press(KeyCode::Char('l'), KeyModifiers::CONTROL);
    h.interaction.input_page.as_mut().unwrap().apply_model(
        vec![crate::agent::ModelProvider {
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
    for expected in [Some('a'), None, Some('a')] {
        let actions = h.press(KeyCode::Char('A'), KeyModifiers::NONE);
        assert!(
            matches!(actions.as_slice(), [UiAction::PersistConfig(saved)]
            if saved.model_marks.letter("p", "m") == expected)
        );
        assert!(h.interaction.input_page.is_some());
        assert_eq!(
            h.state.lock().unwrap().config.model_marks.letter("p", "m"),
            expected
        );
        assert_eq!(h.interaction.input.buf, draft);
    }
    let mut config = h.config.clone();
    config.key_mapping = KeyMapping::from_user_toml("[global]\nprint_help='a'").unwrap();
    h.reload(config);
    h.press(KeyCode::Char('a'), KeyModifiers::NONE);
    assert!(h.interaction.help_visible);
    assert!(h.interaction.input_page.is_some());
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    let mut config = h.config.clone();
    config.key_mapping = KeyMapping::default();
    h.reload(config);
    assert!(
        matches!(h.press(KeyCode::Char('a'), KeyModifiers::NONE).as_slice(),
        [UiAction::Agent(AgentRequest::ModelSet { provider, model, reasoning_effort: None })]
        if provider == "p" && model == "m")
    );
    assert!(h.interaction.input_page.is_none());
    assert_eq!(h.interaction.input.buf, draft);
}

#[test]
fn model_default_effort_command_persists_and_applies_only_on_selection() {
    use crate::agent::{
        ModelDescriptor, ModelProvider, ModelReasoning, ModelSelection, ReasoningEffort,
    };
    let mut h = Harness::new("");
    let original = ModelSelection {
        provider: "p".into(),
        model: "m".into(),
        reasoning_effort: Some("low".into()),
    };
    {
        let mut app = h.state.lock().unwrap();
        app.frontend = crate::FrontendKind::Pi;
        app.catalogs.model_providers = vec![ModelProvider {
            id: "p".into(),
            name: "Provider".into(),
            models: vec![ModelDescriptor {
                id: "m".into(),
                name: "Model".into(),
                description: None,
                context_window: None,
                reasoning: Some(ModelReasoning {
                    efforts: ["low", "high"]
                        .into_iter()
                        .map(|id| ReasoningEffort {
                            id: id.into(),
                            name: id.into(),
                            description: None,
                        })
                        .collect(),
                    default_effort: None,
                }),
            }],
        }];
        app.catalogs.current_model = Some(original.clone());
    }
    h.interaction
        .input
        .restore_text("/model m set-default-effort high".into());
    let actions = h.press(KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        matches!(actions.as_slice(), [UiAction::PersistConfig(config)]
        if config.model_default_efforts.get("p", "m") == Some("high"))
    );
    assert_eq!(
        h.state.lock().unwrap().catalogs.current_model,
        Some(original)
    );
    assert_eq!(
        h.state
            .lock()
            .unwrap()
            .config
            .model_default_efforts
            .get("p", "m"),
        Some("high")
    );
    assert!(h.state.try_lock().is_ok());
    RuntimeController::apply_effect_result(
        EffectResult::ConfigPersisted(Err("read only".into())),
        &h.state,
        Instant::now(),
    );
    assert!(!h.state.lock().unwrap().transcript.nodes().is_empty());

    let reloaded = Config::from_user_toml(&toml::to_string(&h.config).unwrap()).unwrap();
    h.reload(reloaded);
    for (line, expected) in [("/model m", "high"), ("/effort low", "low")] {
        h.interaction.input.restore_text(line.into());
        let actions = h.press(KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            matches!(actions.as_slice(), [UiAction::Agent(AgentRequest::ModelSet {
            provider, model, reasoning_effort: Some(effort),
        })] if provider == "p" && model == "m" && effort == expected)
        );
    }
    assert_eq!(h.config.model_default_efforts.get("p", "m"), Some("high"));
}

#[test]
fn key_mapping_queue_submission_and_cancel_are_remapped_without_fallback() {
    let mut h = Harness::new("[message.working]\nsend_asap='f2'\nsend_after_turn='f3'\n[message]\ncancel_or_interrupt='f4'");
    h.state.lock().unwrap().session.status = crate::SessionStatus::Running;
    h.interaction.input.restore_text("first".into());
    h.press(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.interaction.input.buf, "first");
    h.press(KeyCode::F(2), KeyModifiers::NONE);
    h.interaction.input.restore_text("second".into());
    h.press(KeyCode::F(3), KeyModifiers::NONE);
    assert_eq!(h.interaction.queue.len(), 2);
    assert_eq!(
        h.interaction.queue.entries()[0].delivery,
        crate::interaction::PromptDelivery::Asap
    );
    assert_eq!(
        h.interaction.queue.entries()[1].delivery,
        crate::interaction::PromptDelivery::AfterTurn
    );
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(h.interaction.queue.len(), 2);
    assert!(h.press(KeyCode::F(4), KeyModifiers::NONE).is_empty());
    assert_eq!(h.interaction.queue.len(), 1);
    h.press(KeyCode::F(4), KeyModifiers::NONE);
    assert!(matches!(
        h.press(KeyCode::F(4), KeyModifiers::NONE).as_slice(),
        [UiAction::Agent(AgentRequest::Interrupt)]
    ));
}

#[test]
fn key_mapping_approval_requires_explicit_response_and_paste_obeys_context() {
    let mut h =
        Harness::new("[approval]\nallow='f2'\n[message]\npaste='nop'\n[page.edit]\npaste='f3'");
    assert!(h
        .press(KeyCode::Char('v'), KeyModifiers::CONTROL)
        .is_empty());
    h.interaction.approval = Some(ApprovalCard {
        id: "approval".into(),
        tool_name: "tool".into(),
        reason: String::new(),
    });
    for (code, modifiers) in [
        (KeyCode::Char('y'), KeyModifiers::NONE),
        (KeyCode::Char('l'), KeyModifiers::CONTROL),
        (KeyCode::Enter, KeyModifiers::NONE),
    ] {
        assert!(h.press(code, modifiers).is_empty());
        assert!(h.interaction.approval.is_some());
    }
    assert!(matches!(
        h.press(KeyCode::F(2), KeyModifiers::NONE).as_slice(),
        [UiAction::Agent(AgentRequest::ApprovalAnswer {
            allow: true,
            ..
        })]
    ));
    h.interaction.input_page = Some(InputPageSession::settings(crate::settings::SettingsState {
        editing: Some(crate::settings::Edit::Input { buf: String::new() }),
        ..Default::default()
    }));
    assert!(h
        .press(KeyCode::Char('v'), KeyModifiers::CONTROL)
        .is_empty());
    assert!(matches!(
        h.press(KeyCode::F(3), KeyModifiers::NONE).as_slice(),
        [UiAction::ReadClipboard]
    ));
    for c in "hjklq".chars() {
        h.press(KeyCode::Char(c), KeyModifiers::NONE);
    }
    let InputPage::Settings(settings) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert!(
        matches!(&settings.editing, Some(crate::settings::Edit::Input { buf }) if buf == "hjklq")
    );
    assert!(h.interaction.input.buf.is_empty());
}

#[test]
fn key_mapping_page_browse_and_text_edit_do_not_use_disabled_defaults() {
    let mut h = Harness::new("[page]\nconfirm='f2'\nmove_down='f3'\nback='f4'\n[page.edit]\ndelete_backward='nop'\ncancel='f5'");
    h.interaction.input_page = Some(InputPageSession::login());
    h.press(KeyCode::Enter, KeyModifiers::NONE);
    h.press(KeyCode::Down, KeyModifiers::NONE);
    let InputPage::Login(login) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert!(matches!(login.page, crate::login::Page::Menu));
    assert_eq!(login.pos, 0);
    h.press(KeyCode::F(3), KeyModifiers::NONE);
    h.press(KeyCode::F(2), KeyModifiers::NONE);
    let InputPage::Login(login) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert!(matches!(login.page, crate::login::Page::ProxyList));
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    let InputPage::Login(login) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert!(matches!(login.page, crate::login::Page::ProxyList));
    h.press(KeyCode::F(4), KeyModifiers::NONE);
    let InputPage::Login(login) = &mut h.interaction.input_page.as_mut().unwrap().page else {
        panic!()
    };
    assert!(matches!(login.page, crate::login::Page::Menu));
    login.editing = Some("secret".into());
    h.press(KeyCode::Backspace, KeyModifiers::NONE);
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    h.press(KeyCode::Char('k'), KeyModifiers::ALT);
    let InputPage::Login(login) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert_eq!(login.editing.as_deref(), Some("secret"));
    h.press(KeyCode::F(5), KeyModifiers::NONE);
    let InputPage::Login(login) = &h.interaction.input_page.as_ref().unwrap().page else {
        panic!()
    };
    assert!(login.editing.is_none());
}

#[test]
fn key_mapping_composer_search_and_suggestion_have_independent_bindings() {
    let mut h = Harness::new("[message]\nhistory_search='f2'\n[message.edit]\ndelete_backward=[]\n[message.suggest]\naccept='f3'\ncancel='f4'");
    h.interaction.input.restore_text("keep".into());
    h.press(KeyCode::Backspace, KeyModifiers::NONE);
    h.press(KeyCode::Char('x'), KeyModifiers::SUPER);
    assert_eq!(h.interaction.input.buf, "keep");
    h.interaction.input.history = vec!["query result".into()];
    h.press(KeyCode::F(2), KeyModifiers::NONE);
    h.press(KeyCode::Char('q'), KeyModifiers::NONE);
    assert_eq!(h.interaction.input.search.as_ref().unwrap().query, "q");
    h.press(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(h.interaction.input.buf, "query result");
    h.press(KeyCode::Char('c'), KeyModifiers::CONTROL);
    for c in "/help".chars() {
        h.press(KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert!(h.interaction.input.suggest.is_some());
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    h.press(KeyCode::Enter, KeyModifiers::NONE);
    assert!(h.interaction.input.suggest.is_some());
    h.press(KeyCode::F(4), KeyModifiers::NONE);
    assert!(h.interaction.input.suggest.is_none());
    h.press(KeyCode::Char('c'), KeyModifiers::CONTROL);
    for c in "/help".chars() {
        h.press(KeyCode::Char(c), KeyModifiers::NONE);
    }
    h.press(KeyCode::F(3), KeyModifiers::NONE);
    assert!(h.interaction.input.suggest.is_none());
    assert!(h.interaction.help_visible);
    assert!(h.state.lock().unwrap().transcript.nodes().is_empty());
}

#[test]
fn key_mapping_reload_is_atomic_and_not_persisted() {
    let mut h = Harness::new("[global]\nchoose_model='f1'");
    let original = h.config.key_mapping.clone();
    let invalid = Config {
        key_mapping_error: Some("key_mapping.toml: invalid test binding".into()),
        ..Config::default()
    };
    h.reload(invalid);
    assert_eq!(h.config.key_mapping, original);
    assert_eq!(h.interaction.input.key_mapping, original);
    assert!(h.state.lock().unwrap().transcript.nodes().iter().any(|node| matches!(&node.item,
        crate::display::DisplayItem::Block(block) if block.content.contains("invalid test binding"))));
    let valid = Config {
        key_mapping: KeyMapping::from_user_toml("[global]\nchoose_model='f2'").unwrap(),
        ..Config::default()
    };
    h.reload(valid);
    assert_eq!(
        h.config.key_mapping.resolve(
            Scope::Global,
            &KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)
        ),
        Some(Action::ChooseModel)
    );
    assert!(h.press(KeyCode::F(1), KeyModifiers::NONE).is_empty());
    let persisted = toml::to_string(&h.config).unwrap();
    assert!(!persisted.contains("key_mapping"));
    h.reload(Config::default());
    assert_eq!(h.config.key_mapping, KeyMapping::default());
}

fn reading_navigation_fixture(mapping: &str, items: bool) -> Harness {
    let mut h = Harness::new(mapping);
    for index in 0..48 {
        let text = if index < 3 {
            format!("Block {index}: [link](https://example.com/{index})")
        } else {
            format!(
                "Block {index}: {}",
                "wrapped content ".repeat(index % 5 + 1)
            )
        };
        h.state.lock().unwrap().push_local_markdown(text);
    }
    h.render();
    h.press(KeyCode::Char('r'), KeyModifiers::CONTROL);
    for _ in 0..48 {
        h.press(KeyCode::Char('k'), KeyModifiers::NONE);
    }
    if items {
        h.press(KeyCode::Char('l'), KeyModifiers::NONE);
        assert!(h
            .state
            .lock()
            .unwrap()
            .reading
            .as_ref()
            .unwrap()
            .item_cursor
            .is_some());
    }
    h
}

fn reading_navigation_snapshot(h: &Harness) -> (usize, bool, ScrollState) {
    let app = h.state.lock().unwrap();
    let reading = app.reading.as_ref().unwrap();
    (
        app.reading_document
            .position(&reading.block_cursor)
            .unwrap(),
        reading.item_cursor.is_some(),
        h.interaction.scroll,
    )
}

#[test]
fn key_mapping_reading_fast_movement_matches_fifteen_cursor_steps() {
    for items in [false, true] {
        let mut fast = reading_navigation_fixture("", items);
        let mut ordinary = reading_navigation_fixture("", items);
        for (page, step) in [
            (KeyCode::PageDown, 'j'),
            (KeyCode::PageDown, 'j'),
            (KeyCode::PageDown, 'j'),
            (KeyCode::PageDown, 'j'),
            (KeyCode::PageDown, 'j'),
            (KeyCode::PageUp, 'k'),
            (KeyCode::PageUp, 'k'),
            (KeyCode::PageUp, 'k'),
            (KeyCode::PageUp, 'k'),
            (KeyCode::PageUp, 'k'),
        ] {
            fast.press(page, KeyModifiers::NONE);
            for _ in 0..15 {
                ordinary.press(KeyCode::Char(step), KeyModifiers::NONE);
            }
            assert_eq!(
                reading_navigation_snapshot(&fast),
                reading_navigation_snapshot(&ordinary)
            );
        }
        assert_eq!(reading_navigation_snapshot(&fast).0, 0);
    }
}

#[test]
fn key_mapping_reading_fast_movement_remaps_and_disables_without_global_fallback() {
    for source in [
        "[read_mode]\nmove_up_fast='nop'\nmove_down_fast=[]",
        "[read_mode]\nmove_up_fast='f2'\nmove_down_fast='f3'\n[global]\npage_down='f3'",
    ] {
        let mut h = reading_navigation_fixture(source, true);
        let before = reading_navigation_snapshot(&h);
        h.press(KeyCode::PageDown, KeyModifiers::NONE);
        h.press(KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(reading_navigation_snapshot(&h), before);
        if source.contains("f3") {
            h.press(KeyCode::F(3), KeyModifiers::NONE);
            assert_eq!(reading_navigation_snapshot(&h).0, 15);
            assert!(
                !reading_navigation_snapshot(&h).1,
                "crossing an itemless Block leaves Item mode"
            );
            h.press(KeyCode::F(2), KeyModifiers::NONE);
            assert_eq!(reading_navigation_snapshot(&h).0, 0);
        }
    }
    let mapping = KeyMapping::default();
    assert_eq!(
        route_terminal_event_with_mapping(
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            TerminalFocus::default(),
            &mapping
        ),
        TerminalRoute::TranscriptPage { up: false }
    );
    assert_eq!(
        route_terminal_event_with_mapping(
            Event::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            TerminalFocus {
                reading_view_open: true,
                input_page_open: true,
                ..TerminalFocus::default()
            },
            &mapping
        ),
        TerminalRoute::TranscriptPage { up: false }
    );
}

#[test]
fn key_mapping_reading_exit_and_item_return_preserve_complete_source() {
    let mut h =
        Harness::new("[read_mode]\ncopy_block=['f2','ctrl-v']\n[read_mode.item]\nmove_up='nop'");
    let source = "A [link](https://example.com) with complete source.";
    h.state
        .lock()
        .unwrap()
        .push_local_markdown(source.to_owned());
    h.interaction.input.paste("preserved\ndraft");
    let draft = h.interaction.input.buf.clone();
    h.render();
    h.press(KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(h.state.lock().unwrap().reading.is_some());
    h.press(KeyCode::Char('l'), KeyModifiers::NONE);
    assert!(h
        .state
        .lock()
        .unwrap()
        .reading
        .as_ref()
        .unwrap()
        .item_cursor
        .is_some());
    assert!(h.press(KeyCode::Char('y'), KeyModifiers::NONE).is_empty());
    assert!(
        matches!(h.press(KeyCode::F(2), KeyModifiers::NONE).as_slice(), [UiAction::WriteClipboard(text)] if text == source)
    );
    assert!(
        matches!(h.press(KeyCode::Char('v'), KeyModifiers::CONTROL).as_slice(), [UiAction::WriteClipboard(text)] if text == source)
    );
    assert!(!RuntimeController::apply_effect_result(
        EffectResult::ClipboardRead(Ok(ClipboardPaste::Text("late paste".into()))),
        &h.state,
        Instant::now()
    ));
    h.press(KeyCode::Backspace, KeyModifiers::NONE);
    assert!(h
        .state
        .lock()
        .unwrap()
        .reading
        .as_ref()
        .unwrap()
        .item_cursor
        .is_none());
    h.press(KeyCode::Char('l'), KeyModifiers::NONE);
    h.press(KeyCode::Esc, KeyModifiers::NONE);
    assert!(h.state.lock().unwrap().reading.is_none());
    assert_eq!(h.interaction.input.buf, draft);
    h.press(KeyCode::Char('r'), KeyModifiers::CONTROL);
    h.press(KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(h.state.lock().unwrap().reading.is_none());
}

#[test]
fn slash_help_opens_the_modal_without_a_transcript_item() {
    let mut h = Harness::new("");
    for key in "/help".chars() {
        h.press(KeyCode::Char(key), KeyModifiers::NONE);
    }
    h.press(KeyCode::Enter, KeyModifiers::NONE);

    assert!(h.interaction.help_visible);
    assert!(h.state.lock().unwrap().transcript.nodes().is_empty());
}

#[test]
fn key_mapping_help_uses_effective_labels_and_full_screen_navigation() {
    let mut h = Harness::new(
        "[global]\nprint_help='f1'\n[message]\npaste='nop'\n[full_screen]\nmove_down='x'",
    );
    for language in [Language::English, Language::SimplifiedChinese] {
        h.config.language = language;
        let markdown = crate::help::markdown(&h.config);
        assert!(markdown.contains("`F1`"));
        assert!(markdown.contains("`—`"));
        assert!(!markdown.contains("key.action."));
    }
    h.press(KeyCode::Char('h'), KeyModifiers::CONTROL);
    assert!(!h.interaction.help_visible);
    h.press(KeyCode::F(1), KeyModifiers::NONE);
    assert!(h.interaction.help_visible);
    h.render();
    assert!(h.interaction.help_scroll.viewport_rows() > 0);
    h.press(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(h.interaction.help_scroll.offset(), 1);
    h.press(KeyCode::F(1), KeyModifiers::NONE);
    assert!(!h.interaction.help_visible);
    h.press(KeyCode::F(1), KeyModifiers::NONE);
    assert!(h.interaction.help_visible);
    assert_eq!(h.interaction.help_scroll.offset(), 0);
}
