use super::*;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use e_tui::runtime::sessions::{
    SessionId, SidePreparationResult, SideSessionCommand, SideSessionKind, SideSessionSeed,
};
use e_tui::viewport::ConversationViewport;
use ratatui::layout::Rect;

struct Settings<'a> {
    config: &'a mut Config,
    themes: &'a mut Vec<ThemeFile>,
    theme: &'a mut e_tui::Theme,
}

struct PreviewRestore {
    width: e_tui::config::PaneWidthPercent,
    fullscreen: bool,
}

struct Conversation {
    id: SessionId,
    state: Arc<std::sync::Mutex<RuntimeState>>,
    worker: SessionWorker,
    ports: PiRuntimePorts,
    presentation: e_tui::ui::Presentation,
    loader: e_pi::session_index::SessionLoader,
    availability: Option<e_pi::tips::Availability>,
    spinner: Option<Instant>,
    ready: bool,
    failed: bool,
    failure: Option<String>,
    editor: Option<String>,
    artifact: Option<e_pi::side::PreparedSideFile>,
    pending_config: Option<Config>,
    saved_preview_width: Option<e_tui::config::PaneWidthPercent>,
}

impl Conversation {
    fn start(
        id: SessionId,
        launch: PiLaunchOptions,
        config: &Config,
        seed: Option<SideSessionSeed>,
    ) -> anyhow::Result<Self> {
        let mut state = RuntimeState::default();
        state.tips.register_tips(e_pi::tips::definitions())?;
        state.frontend = e_tui::FrontendKind::Pi;
        state.config = config.clone();
        state.interaction = e_tui::InteractionModel::new(config);
        if let Some(error) = &config.key_mapping_error {
            state.push_error_message(error.clone());
        }
        let state = Arc::new(std::sync::Mutex::new(state));
        let history = Arc::new(std::sync::Mutex::new(
            e_pi::execution_history_store::HistoryRecorder::new("e-pi"),
        ));
        let ports = PiRuntimePorts {
            persist_session_enabled: seed.is_none(),
            history: Arc::clone(&history),
            shell: Default::default(),
        };
        state.lock().unwrap().shell.name = ports.shell.name();
        let editor = seed.as_ref().map(|seed| seed.editor_text.clone());
        let worker = SessionWorker::start_seeded(
            id,
            launch,
            Arc::clone(&state),
            history,
            config.error_auto_retry,
            seed,
        );
        Ok(Self {
            id,
            state,
            worker,
            ports,
            presentation: Default::default(),
            loader: Default::default(),
            availability: None,
            spinner: None,
            ready: false,
            failed: false,
            failure: None,
            editor,
            artifact: None,
            pending_config: None,
            saved_preview_width: None,
        })
    }

    async fn effects(
        &mut self,
        mut actions: Vec<e_tui::UiAction>,
        scheduler: &mut FrameScheduler,
    ) -> e_tui::runtime::EffectExecution {
        for action in &mut actions {
            if let e_tui::UiAction::PersistConfig(config) = action {
                self.pending_config = Some(config.clone());
                if let Some(width) = self.saved_preview_width {
                    config.message_pane_percent = width;
                }
            }
        }
        let requests = self.worker.requests();
        let mut agent = PiAgentPort {
            requests: &requests,
        };
        execute_ui_actions(actions, &mut agent, scheduler, &mut self.ports).await
    }

    async fn agent(
        &mut self,
        event: e_tui::AgentEvent,
        scheduler: &mut FrameScheduler,
    ) -> Result<bool, String> {
        let mut interaction = std::mem::take(&mut self.state.lock().unwrap().interaction);
        let actions = RuntimeController::apply_agent(
            event,
            &self.state,
            &mut RuntimeUiState {
                scroll: &mut interaction.scroll,
                input: &mut interaction.input,
                input_page: &mut interaction.input_page,
                approval: &mut interaction.approval,
                question: &mut interaction.question,
                queue: &mut interaction.queue,
            },
        );
        self.state.lock().unwrap().interaction = interaction;
        let execution = self.effects(actions, scheduler).await;
        for result in execution.completed {
            RuntimeController::apply_effect_result(result, &self.state, Instant::now());
        }
        if let Some(error) = execution.fatal {
            return Err(error);
        }
        Ok(execution.quit)
    }

    async fn terminal(
        &mut self,
        event: Event,
        size: TerminalSize,
        settings: Settings<'_>,
        scheduler: &mut FrameScheduler,
    ) -> Result<bool, String> {
        let Settings {
            config,
            themes,
            theme,
        } = settings;
        let (focus, mcp) = {
            let state = self.state.lock().unwrap();
            (
                TerminalFocus {
                    help_visible: state.interaction.help_visible,
                    input_page_open: state.interaction.input_page.is_some(),
                    approval_open: !state.is_new_conversation()
                        && state.interaction.approval.is_some(),
                    reading_view_open: state.reading.is_some(),
                    history_view_open: state.history_page.is_some(),
                },
                state.mcp.is_some(),
            )
        };
        let route = if mcp {
            e_tui::runtime::input::route_modal_terminal_event(event, focus, &config.key_mapping)
        } else {
            route_terminal_event(event, focus, &config.key_mapping)
        };
        let mut interaction = std::mem::take(&mut self.state.lock().unwrap().interaction);
        let mut ui = TerminalUiState {
            scroll: &mut interaction.scroll,
            input: &mut interaction.input,
            input_page: &mut interaction.input_page,
            help_visible: &mut interaction.help_visible,
            help_scroll: &mut interaction.help_scroll,
            notice: &mut interaction.notice,
            mouse_selection: &mut interaction.mouse_selection,
            pane_resize: &mut interaction.pane_resize,
            approval: &mut interaction.approval,
            question: &mut interaction.question,
            queue: &mut interaction.queue,
            config,
            themes,
            theme,
        };
        let actions = RuntimeController::apply_terminal_route(
            route,
            size,
            self.ports.now(),
            &self.state,
            self.presentation.selection_frame(),
            &mut ui,
        );
        self.state.lock().unwrap().interaction = interaction;
        let execution = self.effects(actions, scheduler).await;
        for result in execution.completed {
            match result {
                EffectResult::ConfigReloaded {
                    config: loaded,
                    themes: loaded_themes,
                } => {
                    let mut interaction =
                        std::mem::take(&mut self.state.lock().unwrap().interaction);
                    RuntimeController::apply_reloaded_config(
                        *loaded,
                        loaded_themes,
                        &self.state,
                        &mut TerminalUiState {
                            scroll: &mut interaction.scroll,
                            input: &mut interaction.input,
                            input_page: &mut interaction.input_page,
                            help_visible: &mut interaction.help_visible,
                            help_scroll: &mut interaction.help_scroll,
                            notice: &mut interaction.notice,
                            mouse_selection: &mut interaction.mouse_selection,
                            pane_resize: &mut interaction.pane_resize,
                            approval: &mut interaction.approval,
                            question: &mut interaction.question,
                            queue: &mut interaction.queue,
                            config,
                            themes,
                            theme,
                        },
                    );
                    self.state.lock().unwrap().interaction = interaction;
                    self.pending_config = Some(config.clone());
                }
                other => {
                    RuntimeController::apply_effect_result(other, &self.state, Instant::now());
                }
            }
        }
        if let Some(error) = execution.fatal {
            return Err(error);
        }
        Ok(execution.quit)
    }

    fn error(&self, error: impl Into<String>) {
        self.state.lock().unwrap().push_error_message(error.into());
    }

    fn fail(&mut self, error: String) {
        self.error(error.clone());
        self.failure = Some(error);
        self.failed = true;
        self.worker.request_shutdown();
        let mut state = self.state.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        state.settle_turn(now, false);
        state.session.active_compaction = None;
        state.session.status = e_tui::SessionStatus::Idle;
    }

    async fn shutdown(mut self) {
        self.ports.shell.shutdown().await;
        self.worker.shutdown().await;
        let history = Arc::clone(&self.ports.history);
        let _ = tokio::task::spawn_blocking(move || history.lock().unwrap().shutdown()).await;
        if let Some(artifact) = self.artifact {
            artifact.cleanup().await;
        }
    }
}

enum Inbound {
    Worker(Option<e_tui::runtime::sessions::SessionEnvelope<SessionWorkerEvent>>),
    Shell(EffectResult),
    Resume(e_tui::resume::ResumeBatch),
}

async fn next_inbound(conversations: &mut [Conversation]) -> (SessionId, Inbound) {
    let Some((first, rest)) = conversations.split_first_mut() else {
        return std::future::pending().await;
    };
    tokio::select! {
        value = first.worker.recv(), if !first.failed => (first.id, Inbound::Worker(value)),
        value = first.ports.shell.next() => (first.id, Inbound::Shell(value)),
        value = first.loader.next_batch() => (first.id, Inbound::Resume(value)),
        value = Box::pin(next_inbound(rest)) => value,
    }
}

fn conversation_area(
    area: Rect,
    pane_percent: e_tui::config::PaneWidthPercent,
    fullscreen: bool,
) -> (Option<Rect>, Option<Rect>) {
    use e_tui::ui::screen::{layout, ScreenLayout};
    match layout(area, pane_percent, fullscreen) {
        ScreenLayout::MainOnly(main) => (Some(main), None),
        ScreenLayout::Split { main, preview } => (Some(main), Some(preview)),
        ScreenLayout::PreviewOnly(preview) => (None, Some(preview)),
    }
}

fn allocate(
    workspace: &SessionWorkspace<()>,
    conversations: &mut [Conversation],
    area: Rect,
    config: &Config,
) {
    let (pane_percent, fullscreen, reading) = conversations
        .iter()
        .find(|slot| slot.id == workspace.focused())
        .map(|slot| {
            let state = slot.state.lock().unwrap();
            (
                state.message_pane_percent(),
                state.preview_fullscreen() && state.history_page.is_none(),
                state.reading.is_some(),
            )
        })
        .unwrap_or((config.message_pane_percent, false, false));
    let (main, preview) = conversation_area(area, pane_percent, fullscreen);
    let split = !reading && workspace.side().is_some() && main.is_some_and(|area| area.width >= 49);
    for slot in conversations {
        let focused = slot.id == workspace.focused();
        let mut state = slot.state.lock().unwrap();
        state.side_session_opening = workspace
            .pending()
            .is_some_and(|creation| creation.source() == slot.id);
        state.viewport = if workspace.side().is_none() {
            None
        } else {
            let main = main.and_then(|area| {
                if split {
                    let left = area.width / 2;
                    let right = area.width - left - 1;
                    Some(if slot.id == workspace.main().id() {
                        Rect::new(area.x, area.y, left, area.height)
                    } else {
                        Rect::new(area.x + left + 1, area.y, right, area.height)
                    })
                } else {
                    focused.then_some(area)
                }
            });
            Some(ConversationViewport {
                main,
                preview: focused.then_some(preview).flatten(),
                focused,
            })
        };
    }
}

pub(super) async fn run_frontend(
    mut launch: PiLaunchOptions,
    herdr: Option<&HerdrReporter>,
) -> anyhow::Result<()> {
    let mut phases =
        e_tui::profile::PhaseTimers::new(std::env::var("DSH_TUI_TIMING").as_deref() == Ok("1"));
    #[allow(unused_variables)]
    let _tracy = e_tui::profile::start_tracy(std::env::var("DSH_TUI_TRACY").as_deref() == Ok("1"));
    let mut config = e_pi::config::load();
    let mut themes = e_pi::effects::load_themes(&e_pi::config::themes_dir());
    config.resolved_theme = e_tui::theme::resolve(&config.theme, &themes);
    let mut theme = config.theme();
    phases.mark("config load");
    e_tui::syntax::warm_up();
    phases.mark("syntax warm-up");
    if launch.session.is_none() && config.remember_last_session {
        launch.session = e_pi::config::StateFile::load().last_session_path;
    }
    let template = launch.clone();
    let sync_output = std::env::var("DSHE_DISABLE_SYNC_OUTPUT").as_deref() != Ok("1");
    #[cfg(windows)]
    let mut terminal =
        TerminalOwner::new_with_options(platform_input::enable_virtual_terminal_input, sync_output)
            .context("initialize terminal")?;
    #[cfg(not(windows))]
    let mut terminal =
        TerminalOwner::new_with_options(|| Ok(()), sync_output).context("initialize terminal")?;
    #[cfg(windows)]
    let mut events = ProductionTerminalEvents::new(platform_input::native_mods);
    #[cfg(not(windows))]
    let mut events = ProductionTerminalEvents::new();
    phases.mark("terminal setup");
    let mut workspace = SessionWorkspace::new(());
    let mut conversations = vec![Conversation::start(
        workspace.focused(),
        launch,
        &config,
        None,
    )?];
    phases.mark("Pi worker scheduled");
    let mut frame_metrics = e_tui::profile::FrameMetrics::new(
        std::env::var("DSHE_FRAME_TIMING").as_deref() == Ok("1"),
        240,
        120,
    );
    let mut pending_update = std::time::Duration::ZERO;
    let mut first_draw = true;
    let mut scheduler = FrameScheduler::new(Instant::now());
    let mut cleanup = tokio::task::JoinSet::new();
    let mut confirming: Option<SessionId> = None;
    let mut pending_source = None;
    let mut preview_restore: Option<PreviewRestore> = None;
    let mut fatal = None;

    'running: loop {
        let _zone = e_tui::tracy_zone!("main loop");
        let area = terminal
            .size()
            .map(|size| Rect::new(0, 0, size.width, size.height))
            .unwrap_or(Rect::new(0, 0, 80, 24));
        let size = TerminalSize {
            width: area.width,
            height: area.height,
        };
        allocate(&workspace, &mut conversations, area, &config);
        let mut deadline = scheduler.deadline();
        for slot in &mut conversations {
            slot.worker.set_error_auto_retry(config.error_auto_retry);
            let mut state = slot.state.lock().unwrap();
            if let Some(availability) = &slot.availability {
                availability.apply(&mut state);
                state.side_sessions_available = availability.side_sessions;
            }
            if workspace.get(slot.id).is_none() {
                continue;
            }
            let now = Instant::now();
            if slot.spinner.is_none() && animation_active(&state, now) {
                slot.spinner = Some(now);
            }
            deadline = e_tui::reveal::earliest_deadline(deadline, slot.spinner);
            deadline = e_tui::reveal::earliest_deadline(deadline, state.reveal_deadline());
            deadline = e_tui::reveal::earliest_deadline(
                deadline,
                state
                    .interaction
                    .notice
                    .deadline(state.config.copy_toast_secs),
            );
            if slot.id == workspace.focused() {
                if let Some(herdr) = herdr {
                    herdr.report(HerdrStatus::from_runtime(&state));
                }
            }
            drop(state);
            RuntimeController::reconcile_tips(&slot.state, size, &mut scheduler, now);
        }
        deadline = e_tui::reveal::earliest_deadline(deadline, scheduler.deadline());
        let mut close = None;
        let mut close_confirmed = false;
        let mut command = None;
        let mut prepared = None;
        let mut side_attached = false;
        let mut update_started = None;
        tokio::select! {
            (id, inbound) = next_inbound(&mut conversations) => {
                update_started = Some(Instant::now());
                let slot = conversations.iter_mut().find(|slot| slot.id == id).unwrap();
                match inbound {
                    Inbound::Worker(Some(envelope)) if envelope.session == id => match envelope.payload {
                        SessionWorkerEvent::Agent(event) => {
                            match slot.agent(*event, &mut scheduler).await {
                                Ok(true) => close = Some(id),
                                Err(error) => slot.fail(error),
                                _ => {}
                            }
                        }
                        SessionWorkerEvent::Availability(value) => slot.availability = Some(value),
                        SessionWorkerEvent::Ready => {
                            slot.ready = true;
                            if id == workspace.main().id() { phases.mark("Pi session ready"); }
                            if let Some(creation) = workspace.pending().filter(|creation| creation.session() == id) {
                                if workspace.finish_side(creation, ()).is_ok() {
                                    side_attached = true;
                                    if let Some(artifact) = slot.artifact.take() { artifact.commit(); }
                                    slot.ports.persist_session_enabled = true;
                                    let identity = slot.state.lock().unwrap().session.session_id.clone();
                                    if let Some(identity) = identity { slot.ports.persist_session_id(identity); }
                                    if let Some(text) = slot.editor.take() { slot.state.lock().unwrap().interaction.input.restore_text(text); }
                                }
                            }
                        }
                        SessionWorkerEvent::SidePrepared { creation, result, artifact } => prepared = Some((creation, result, artifact)),
                        SessionWorkerEvent::Fatal(error) => slot.fail(error),
                    },
                    Inbound::Worker(_) => slot.fail("Pi session worker closed".into()),
                    Inbound::Shell(result) => { RuntimeController::apply_effect_result(result, &slot.state, Instant::now()); }
                    Inbound::Resume(batch) => { RuntimeController::apply_resume_batch_with_parents(batch, slot.loader.parents().unwrap_or(&Default::default()), &slot.state); }
                }
                if slot.failed {
                    if workspace.pending().is_some_and(|creation| creation.session() == id) {
                        let creation = workspace.pending().unwrap();
                        let reason = slot.failure.clone().unwrap_or_else(|| "unknown failure".into());
                        workspace.cancel_side(creation);
                        if let Some(source) = conversations.iter().find(|slot| slot.id == creation.source()) { source.error(format!("Side session initialization failed: {reason}")); }
                        let index = conversations.iter().position(|slot| slot.id == id).unwrap();
                        cleanup.spawn(conversations.remove(index).shutdown());
                    } else if workspace.side().is_none() {
                        fatal = slot.failure.clone(); break 'running;
                    }
                }
                scheduler.request(DirtyReason::Content, Instant::now());
            }
            event = events.next_event() => {
                update_started = Some(Instant::now());
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => { fatal = Some(error); break 'running; }
                    None => break 'running,
                };
                let cancel_creation = matches!(&event, Event::Key(key) if key.code == KeyCode::Esc && key.kind != KeyEventKind::Release)
                    && workspace.pending().is_some()
                    && !conversations.iter().any(|slot| slot.state.lock().unwrap().interaction.input_page.is_some());
                if cancel_creation {
                    let creation = workspace.pending().unwrap();
                    workspace.cancel_side(creation);
                    pending_source = None;
                    if let Some(source) = conversations.iter().find(|slot| slot.id == creation.source()) { source.worker.cancel_side(creation); }
                    if let Some(index) = conversations.iter().position(|slot| slot.id == creation.session()) { cleanup.spawn(conversations.remove(index).shutdown()); }
                } else if let Some(id) = confirming {
                    if let Event::Key(key) = event {
                        if key.kind != KeyEventKind::Release {
                            match key.code { KeyCode::Char('y' | 'Y') => { confirming = None; close = Some(id); close_confirmed = true; }, KeyCode::Esc | KeyCode::Char('n' | 'N') => confirming = None, _ => {} }
                        }
                    }
                } else {
                    let mut target = workspace.focused();
                    let mut focus_changed = false;
                    let modal = conversations.iter().find(|slot| slot.id == target).is_some_and(|slot| { let state = slot.state.lock().unwrap(); state.interaction.help_visible || state.mcp.is_some() });
                    if !modal {
                        if let Event::Mouse(mouse) = &event {
                            if let Some(slot) = conversations.iter().find(|slot| workspace.get(slot.id).is_some() && slot.state.lock().unwrap().viewport.and_then(|view| view.main).is_some_and(|area| area.contains((mouse.column, mouse.row).into()))) {
                                target = slot.id;
                                if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                                    focus_changed = target != workspace.focused();
                                    let _ = workspace.focus(target);
                                }
                            }
                        }
                    }
                    if matches!(&event, Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Drag(_) | MouseEventKind::Up(_) | MouseEventKind::Moved)) {
                        if let Some(captured) = conversations.iter().find(|slot| { let state = slot.state.lock().unwrap(); state.interaction.mouse_selection.is_dragging() || state.interaction.pane_resize.is_active() }) { target = captured.id; }
                    }
                    let slot = conversations.iter_mut().find(|slot| slot.id == target).unwrap();
                    if !focus_changed {
                        match slot.terminal(event, size, Settings { config: &mut config, themes: &mut themes, theme: &mut theme }, &mut scheduler).await {
                            Ok(true) => close = Some(target),
                            Err(error) => slot.fail(error),
                            _ => {}
                        }
                    }
                    if slot.failed && workspace.side().is_none() { fatal = slot.failure.clone(); break 'running; }
                    command = slot.state.lock().unwrap().side_commands.pop_front().map(|command| (target, command));
                }
                scheduler.request(DirtyReason::Interactive, Instant::now());
            }
            _ = wait_for_deadline(deadline) => {
                update_started = Some(Instant::now());
                let now = Instant::now();
                for slot in &mut conversations {
                    let mut state = slot.state.lock().unwrap();
                    if slot.spinner.is_some_and(|deadline| deadline <= now) {
                        tick_spinners(&mut state, now);
                        slot.spinner = animation_active(&state, now).then(|| now + animation_interval(config.spinner_frame_ms));
                    }
                    state.tick_reveals(now);
                    state.interaction.notice.expire(config.copy_toast_secs, now);
                }
                scheduler.request(DirtyReason::Content, now);
            }
            _ = cleanup.join_next(), if !cleanup.is_empty() => {}
        }
        if side_attached {
            let fullscreen = conversations
                .iter()
                .find(|slot| slot.id == workspace.main().id())
                .is_some_and(|slot| slot.state.lock().unwrap().preview.fullscreen);
            let width = config.message_pane_percent;
            preview_restore = Some(PreviewRestore { width, fullscreen });
            config.message_pane_percent = e_tui::config::PaneWidthPercent::from_basis_points(
                e_tui::config::PaneWidthPercent::MAX_BASIS_POINTS,
            )
            .expect("maximum pane percentage is valid");
            for slot in &mut conversations {
                slot.saved_preview_width = Some(width);
                let mut state = slot.state.lock().unwrap();
                state.config.message_pane_percent = config.message_pane_percent;
                state.preview.fullscreen = false;
                state.interaction.pane_resize.cancel();
                state.interaction.mouse_selection.clear();
            }
        }
        let mut config_changed = false;
        for slot in &mut conversations {
            if let Some(updated) = slot.pending_config.take() {
                config = updated;
                config_changed = true;
            }
        }
        if config_changed {
            for slot in &mut conversations {
                let mut interaction = std::mem::take(&mut slot.state.lock().unwrap().interaction);
                RuntimeController::synchronize_config(
                    &config,
                    &slot.state,
                    &mut interaction.input,
                    &mut theme,
                );
                slot.state.lock().unwrap().interaction = interaction;
            }
        }
        if let Some((creation, result, artifact)) = prepared {
            if workspace.pending() != Some(creation) {
                if let Some(artifact) = artifact {
                    cleanup.spawn(artifact.cleanup());
                }
            } else {
                match result {
                    SidePreparationResult::Ready(seed) => {
                        let mut launch = template.clone();
                        launch.session = seed.session_id.clone();
                        launch.cwd = seed.workspace.clone().into();
                        match Conversation::start(creation.session(), launch, &config, Some(seed)) {
                            Ok(mut candidate) => {
                                candidate.artifact = artifact;
                                conversations.push(candidate);
                            }
                            Err(error) => {
                                workspace.cancel_side(creation);
                                if let Some(source) = conversations
                                    .iter()
                                    .find(|slot| slot.id == creation.source())
                                {
                                    source.error(error.to_string());
                                }
                            }
                        }
                    }
                    other => {
                        workspace.cancel_side(creation);
                        if let SidePreparationResult::Failed(error) = other {
                            if let Some(source) = conversations
                                .iter()
                                .find(|slot| slot.id == creation.source())
                            {
                                source.error(error);
                            }
                        }
                    }
                }
            }
        }
        if command.is_none() {
            command = conversations.iter().find_map(|slot| {
                slot.state
                    .lock()
                    .unwrap()
                    .side_commands
                    .pop_front()
                    .map(|command| (slot.id, command))
            });
        }
        if let Some((id, requested)) = command {
            let requested = match requested {
                SideSessionCommand::Toggle if workspace.side().is_some() => {
                    let _ = workspace.focus_other();
                    None
                }
                SideSessionCommand::Toggle => {
                    Some(SideSessionCommand::Create(SideSessionKind::Clone))
                }
                other => Some(other),
            };
            let result = match requested {
                Some(SideSessionCommand::Swap) => {
                    workspace.swap().map_err(|error| error.to_string())
                }
                Some(SideSessionCommand::Close) => {
                    if workspace.side().is_some() {
                        close = Some(id);
                        Ok(())
                    } else {
                        Err("No side session is open".into())
                    }
                }
                Some(SideSessionCommand::Create(kind)) => {
                    if area.width < 49 {
                        Err("Not enough conversation width to open a side session".into())
                    } else {
                        match workspace.begin_side(kind) {
                            Ok(creation) => {
                                let source = conversations
                                    .iter()
                                    .find(|slot| slot.id == creation.source())
                                    .unwrap();
                                match source.worker.prepare_side(creation) {
                                    Ok(()) => {
                                        pending_source = Some((
                                            creation,
                                            source.state.lock().unwrap().session.session_id.clone(),
                                        ));
                                        Ok(())
                                    }
                                    Err(error) => {
                                        workspace.cancel_side(creation);
                                        Err(error)
                                    }
                                }
                            }
                            Err(error) => Err(error.to_string()),
                        }
                    }
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                if let Some(slot) = conversations.iter().find(|slot| slot.id == id) {
                    slot.error(error);
                }
            }
            scheduler.request(DirtyReason::Interactive, Instant::now());
        }
        if let Some(id) = close {
            if workspace.side().is_none() {
                break 'running;
            }
            let index = conversations.iter().position(|slot| slot.id == id).unwrap();
            let busy = {
                let state = conversations[index].state.lock().unwrap();
                state.session.working
                    || state.session.status != e_tui::SessionStatus::Idle
                    || state.shell.active.is_some()
                    || !state.interaction.queue.is_empty()
            };
            // A confirmed target remains stable even if a different conversation becomes focused.
            if busy && !close_confirmed {
                confirming = Some(id);
            } else {
                confirming = None;
                let _ = workspace.close(id);
                cleanup.spawn(conversations.remove(index).shutdown());
                if let Some(saved) = preview_restore.take() {
                    config.message_pane_percent = saved.width;
                    for slot in &mut conversations {
                        slot.saved_preview_width = None;
                        let mut state = slot.state.lock().unwrap();
                        state.config.message_pane_percent = saved.width;
                        state.preview.fullscreen = saved.fullscreen;
                        state.interaction.pane_resize.cancel();
                        state.interaction.mouse_selection.clear();
                    }
                }
                if let Some(survivor) = conversations
                    .iter_mut()
                    .find(|slot| slot.id == workspace.main().id())
                {
                    let identity = survivor.state.lock().unwrap().session.session_id.clone();
                    if let Some(identity) = identity {
                        survivor.ports.persist_session_id(identity);
                    }
                }
            }
            scheduler.request(DirtyReason::Interactive, Instant::now());
        }
        for slot in &mut conversations {
            if workspace.get(slot.id).is_none() || slot.failed {
                continue;
            }
            let effects = RuntimeController::dispatch_next_queued(&slot.state);
            if !effects.is_empty() {
                let execution = slot.effects(effects, &mut scheduler).await;
                for result in execution.completed {
                    RuntimeController::apply_effect_result(result, &slot.state, Instant::now());
                }
                if let Some(error) = execution.fatal {
                    slot.fail(error);
                    if workspace.side().is_none() {
                        fatal = slot.failure.clone();
                        break 'running;
                    }
                }
                if execution.quit {
                    if workspace.side().is_none() {
                        break 'running;
                    }
                    slot.state
                        .lock()
                        .unwrap()
                        .side_commands
                        .push_back(SideSessionCommand::Close);
                    scheduler.request(DirtyReason::Interactive, Instant::now());
                }
            }
            if slot.loader.is_idle() {
                if let Some(request) = RuntimeController::take_resume_request(&slot.state) {
                    let root = e_pi::session_index::project_session_root(std::path::Path::new(
                        &request.workspace,
                    ));
                    slot.loader.start(root, request);
                }
            }
        }
        if let Some((creation, identity)) = &pending_source {
            if workspace.pending() == Some(*creation)
                && conversations
                    .iter()
                    .find(|slot| slot.id == creation.source())
                    .is_none_or(|slot| slot.state.lock().unwrap().session.session_id != *identity)
            {
                let creation = *creation;
                workspace.cancel_side(creation);
                if let Some(source) = conversations
                    .iter()
                    .find(|slot| slot.id == creation.source())
                {
                    source.worker.cancel_side(creation);
                }
                if let Some(index) = conversations
                    .iter()
                    .position(|slot| slot.id == creation.session())
                {
                    cleanup.spawn(conversations.remove(index).shutdown());
                }
            }
        }
        if workspace.pending().is_none() {
            pending_source = None;
        }
        allocate(&workspace, &mut conversations, area, &config);
        if let Some(started) = update_started {
            pending_update += started.elapsed();
        }
        let now = Instant::now();
        if let Some(requested) = scheduler.take_due(now) {
            let _first_zone = if first_draw {
                e_tui::tracy_zone!("first frame")
            } else {
                None
            };
            let mut sample = e_tui::profile::FrameSample {
                scheduler_delay: now.saturating_duration_since(requested),
                update: pending_update,
                ..Default::default()
            };
            let mut candidates = Vec::new();
            let transaction = terminal.draw(|frame| {
                let area = frame.area();
                allocate(&workspace, &mut conversations, area, &config);
                let mut cursor = None;
                let focused_modal = conversations
                    .iter()
                    .find(|slot| slot.id == workspace.focused())
                    .is_some_and(|slot| {
                        let state = slot.state.lock().unwrap();
                        state.interaction.help_visible || state.mcp.is_some()
                    });
                for focused in [false, true] {
                    for slot in &mut conversations {
                        if workspace.get(slot.id).is_none()
                            || (slot.id == workspace.focused()) != focused
                            || (focused_modal && !focused)
                        {
                            continue;
                        }
                        let mut state = slot.state.lock().unwrap();
                        if state
                            .viewport
                            .is_some_and(|view| view.main.is_none() && view.preview.is_none())
                        {
                            continue;
                        }
                        let mut interaction = std::mem::take(&mut state.interaction);
                        let background_mcp = if focused { None } else { state.mcp.take() };
                        if focused_modal {
                            state.viewport = None;
                        }
                        let output = e_tui::ui::render_with_cursor_and_selection(
                            frame,
                            &mut state,
                            &interaction.input,
                            &mut interaction.scroll,
                            &theme,
                            e_tui::ui::RenderOverlays {
                                help_visible: interaction.help_visible,
                                help_scroll: Some(&mut interaction.help_scroll),
                                toast: interaction
                                    .notice
                                    .visible_text(config.copy_toast_secs, Instant::now()),
                                input_page: interaction.input_page.as_mut(),
                                settings: None,
                                login: None,
                                approval: interaction.approval.as_ref(),
                                queue: interaction.queue.entries(),
                                pane_resize: interaction.pane_resize,
                            },
                            &interaction.mouse_selection,
                            &slot.presentation,
                        );
                        if focused {
                            cursor = output.cursor;
                        }
                        candidates.push((slot.id, output.presentation));
                        state.interaction = interaction;
                        if background_mcp.is_some() {
                            state.mcp = background_mcp;
                        }
                        let work = state.render.transcript_cache.take_work_stats();
                        sample.cache_rebuilds += work.rebuilds;
                        sample.cache_patches += work.patches;
                        sample.materialized_rows += work.materialized_rows;
                        let work = state.preview.take_work_stats();
                        sample.preview_rebuilds += work.rebuilds;
                        sample.preview_patches += work.patches;
                        sample.preview_materialized_rows += work.materialized_rows;
                    }
                }
                if confirming.is_some() {
                    use ratatui::widgets::{Block, Borders, Clear, Paragraph};
                    let rect = Rect::new(
                        area.width.saturating_sub(58) / 2,
                        area.height.saturating_sub(3) / 2,
                        area.width.min(58),
                        3.min(area.height),
                    );
                    frame.render_widget(Clear, rect);
                    frame.render_widget(
                        Paragraph::new("Stop and close this busy conversation? [y/N]")
                            .block(Block::default().borders(Borders::ALL))
                            .style(theme.surface.base.style()),
                        rect,
                    );
                    cursor = None;
                }
                cursor
            });
            let transaction = match transaction {
                Ok(transaction) => transaction,
                Err(error) => {
                    fatal = Some(error.to_string());
                    break 'running;
                }
            };
            sample.render = transaction.render;
            sample.draw = std::time::Duration::from_nanos(transaction.io.draw_ns);
            sample.flush = std::time::Duration::from_nanos(transaction.io.flush_ns);
            sample.total = transaction.total;
            sample.changed_cells = transaction.io.changed_cells;
            sample.emitted_bytes = transaction.io.emitted_bytes;
            if let Some(report) = frame_metrics.record(sample) {
                eprintln!("{report}");
            }
            pending_update = std::time::Duration::ZERO;
            if first_draw {
                first_draw = false;
                phases.mark("first frame");
            }
            for (id, candidate) in candidates {
                if let Some(slot) = conversations.iter_mut().find(|slot| slot.id == id) {
                    slot.presentation.commit(
                        candidate,
                        &mut slot.state.lock().unwrap().interaction.mouse_selection,
                    );
                }
            }
            scheduler.complete(Instant::now());
        }
    }
    let commands: Vec<_> = conversations
        .iter()
        .filter(|slot| workspace.get(slot.id).is_some())
        .filter_map(|slot| {
            slot.state
                .lock()
                .unwrap()
                .session
                .session_id
                .as_deref()
                .and_then(resume_command)
        })
        .collect();
    for slot in conversations {
        cleanup.spawn(slot.shutdown());
    }
    while cleanup.join_next().await.is_some() {}
    terminal.restore_terminal().ok();
    for command in commands {
        println!("Resume this session with:\n  {command}");
    }
    if let Some(error) = fatal {
        bail!("{error}");
    }
    Ok(())
}
