#![deny(clippy::significant_drop_in_scrutinee)]

//! `pie` — the Pi coding agent hosted by the kernel-neutral ratatui frontend.
//!
//! Pi remains the authoritative agent runtime. This process owns only the Pi
//! RPC child, protocol adaptation, terminal lifecycle, and frontend runtime.

mod platform_input;

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{collections::VecDeque, path::PathBuf};

use anyhow::{bail, Context};
use e_pi::{
    adapter::{AdapterOutput, PiAdapter},
    process::{PiLaunchOptions, PiProcess, PiProcessEvent, ProjectTrust},
    protocol::RpcCommand,
};
use e_tui::profile::{FrameMetrics, FrameSample};
use e_tui::runtime::{
    animation_active, animation_interval, execute_ui_actions, inbound_budget_remaining,
    is_streaming_delta, route_terminal_event, tick_spinners, wait_for_deadline, AgentRequestPort,
    DirtyReason, FrameScheduler, ProductionTerminalEvents, RuntimeController, RuntimeState,
    RuntimeUiState, TerminalEventPort, TerminalFocus, TerminalLifecyclePort, TerminalOwner,
    TerminalUiState, UiActionPorts,
};
use e_tui::{
    ui::TerminalSize, AgentEvent, AgentRequest, Config, EffectResult, PreviewContent,
    PreviewRequest, ThemeFile,
};

#[derive(Debug, Clone)]
struct Cli {
    launch: PiLaunchOptions,
}

fn help() -> &'static str {
    "pie [--cwd <directory>] [--session <file>] [--approve|--no-approve] [--pi <executable>]\n\nRuns the e-tui frontend against the official `pi --mode rpc` runtime.\nProject trust follows native Pi behavior unless an explicit trust flag is supplied."
}

fn parse_cli_from(mut args: impl Iterator<Item = String>) -> anyhow::Result<Cli> {
    let mut launch = PiLaunchOptions::for_cwd(std::env::current_dir()?);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--cwd" => {
                launch.cwd = PathBuf::from(args.next().context("--cwd requires a directory")?)
            }
            "--session" => {
                launch.session = Some(
                    args.next()
                        .context("--session requires a Pi session file")?,
                )
            }
            "--approve" => launch.trust = ProjectTrust::Approve,
            "--no-approve" => launch.trust = ProjectTrust::Reject,
            "--pi" => launch.executable = args.next().context("--pi requires an executable")?,
            "--help" | "-h" => {
                println!("{}", help());
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("pie {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            option if option.starts_with('-') => {
                bail!("unknown pie option `{option}`\n\n{}", help())
            }
            session if launch.session.is_none() => launch.session = Some(session.to_owned()),
            extra => bail!("unexpected argument `{extra}`\n\n{}", help()),
        }
    }
    Ok(Cli { launch })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli_from(std::env::args().skip(1))?;
    run(cli.launch).await
}

struct PiRuntimePorts;

impl UiActionPorts for PiRuntimePorts {
    fn load_config(&mut self) -> Result<(Config, Vec<ThemeFile>), String> {
        let mut config = e_pi::config::load();
        let themes = e_pi::effects::load_themes(&e_pi::config::themes_dir());
        config.resolved_theme = e_tui::theme::resolve(&config.theme, &themes);
        Ok((config, themes))
    }

    fn persist_config(&mut self, config: &Config) -> Result<(), String> {
        e_pi::config::save(config)
    }

    fn persist_session_id(&mut self, session_id: String) {
        let mut state = e_pi::config::StateFile::load();
        state.last_session_path = Some(session_id);
        state.save();
    }

    fn read_clipboard(&mut self) -> Result<e_tui::ClipboardPaste, String> {
        e_pi::effects::read_clipboard()
    }

    fn write_clipboard(&mut self, text: String) -> Result<(), String> {
        e_pi::effects::write_clipboard(text)
    }

    fn resolve_preview(
        &mut self,
        request: PreviewRequest,
    ) -> impl std::future::Future<Output = Result<PreviewContent, String>> + Send {
        e_pi::effects::resolve_preview(request)
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

struct PiAgentPort<'a> {
    requests: &'a tokio::sync::mpsc::Sender<AgentRequest>,
}

impl AgentRequestPort for PiAgentPort<'_> {
    fn send_agent_request(
        &mut self,
        request: AgentRequest,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send {
        async move {
            self.requests
                .send(request)
                .await
                .map_err(|_| "Pi adapter request channel closed".to_owned())
        }
    }
}

async fn route_output(
    output: AdapterOutput,
    rpc: &tokio::sync::mpsc::Sender<RpcCommand>,
    pending: &mut VecDeque<AgentEvent>,
) -> Result<(), String> {
    for command in output.commands {
        rpc.send(command)
            .await
            .map_err(|_| "Pi RPC stdin channel closed".to_owned())?;
    }
    pending.extend(output.events);
    Ok(())
}

async fn run(mut launch: PiLaunchOptions) -> anyhow::Result<()> {
    let mut phases =
        e_tui::profile::PhaseTimers::new(std::env::var("DSH_TUI_TIMING").as_deref() == Ok("1"));
    #[allow(unused_variables)]
    let _tracy = e_tui::profile::start_tracy(std::env::var("DSH_TUI_TRACY").as_deref() == Ok("1"));
    let _z = e_tui::tracy_zone!("config load");
    let mut config = e_pi::config::load();
    let mut themes = e_pi::effects::load_themes(&e_pi::config::themes_dir());
    config.resolved_theme = e_tui::theme::resolve(&config.theme, &themes);
    drop(_z);
    phases.mark("config load");
    let theme = config.theme();
    let mut app = RuntimeState::default();
    if let Some(error) = &config.key_mapping_error {
        eprintln!("{error}");
        app.push_error_message(error.clone());
    }
    app.frontend = e_tui::FrontendKind::Pi;
    app.config = config.clone();
    app.interaction = e_tui::InteractionModel::new(&config);
    let state = Arc::new(std::sync::Mutex::new(app));
    let mut theme = theme;
    // Syntect's embedded syntax dump has a measurable cold-start cost. Warm it
    // off the render path and overlap that pure CPU work with bridge attach.
    let syntax_warmup = std::thread::spawn(e_tui::syntax::warm_up);

    if launch.session.is_none() && config.remember_last_session {
        launch.session = e_pi::config::StateFile::load().last_session_path;
    }
    let _z = e_tui::tracy_zone!("Pi RPC spawn");
    let mut process = PiProcess::spawn(&launch).await?;
    let rpc = process.sender();
    let mut adapter = PiAdapter::new(
        &launch.cwd,
        e_pi::session_index::project_session_root(&launch.cwd),
    );
    let mut pending_inbound = VecDeque::new();
    route_output(
        AdapterOutput {
            commands: adapter.startup_commands(),
            events: Vec::new(),
        },
        &rpc,
        &mut pending_inbound,
    )
    .await
    .map_err(anyhow::Error::msg)?;
    let (request_tx, mut request_rx) = tokio::sync::mpsc::channel::<AgentRequest>(256);
    drop(_z);
    phases.mark("Pi RPC spawn");
    syntax_warmup
        .join()
        .map_err(|_| anyhow::anyhow!("syntax asset warm-up panicked"))?;
    phases.mark("syntax warm-up");
    let state_r = Arc::clone(&state);

    // ---- event-driven main loop ----
    let sync_output = std::env::var("DSHE_DISABLE_SYNC_OUTPUT").as_deref() != Ok("1");
    #[cfg(windows)]
    let mut terminal =
        TerminalOwner::new_with_options(platform_input::enable_virtual_terminal_input, sync_output)
            .context("initialize terminal")?;
    #[cfg(not(windows))]
    let mut terminal =
        TerminalOwner::new_with_options(|| Ok(()), sync_output).context("initialize terminal")?;
    phases.mark("terminal setup");
    #[cfg(windows)]
    let mut events = ProductionTerminalEvents::new(platform_input::native_mods);
    #[cfg(not(windows))]
    let mut events = ProductionTerminalEvents::new();
    let mut runtime_ports = PiRuntimePorts;
    let mut scheduler = FrameScheduler::new(runtime_ports.now());
    let mut committed_selection_frame = e_tui::SelectionFrame::default();
    let mut spinner_deadline: Option<Instant> = None;
    let mut frame_metrics = FrameMetrics::new(
        std::env::var("DSHE_FRAME_TIMING").as_deref() == Ok("1"),
        240,
        120,
    );
    let mut pending_update_elapsed = Duration::ZERO;
    let mut fatal: Option<String> = None;
    let mut first_draw_done = false;

    'outer: loop {
        let _main_loop_zone = e_tui::tracy_zone!("main loop");
        let mut pending_event = None;
        let mut first_inbound = None;
        let frame_deadline = scheduler.deadline();
        let (reveal_deadline, notice_deadline) = {
            let state = state_r.lock().unwrap();
            (
                state.reveal_deadline(),
                state
                    .interaction
                    .notice
                    .deadline(state.config.copy_toast_secs),
            )
        };
        let animation_deadline =
            e_tui::reveal::earliest_deadline(spinner_deadline, reveal_deadline);
        if let Some(event) = pending_inbound.pop_front() {
            first_inbound = Some(event);
        } else {
            tokio::select! {
                maybe = process.recv() => {
                    match maybe {
                        Some(PiProcessEvent::Record(record)) => {
                            if let Err(error) = route_output(adapter.record(record), &rpc, &mut pending_inbound).await {
                                fatal = Some(error);
                                break 'outer;
                            }
                            first_inbound = pending_inbound.pop_front();
                        }
                        Some(PiProcessEvent::Fatal(error)) => {
                            fatal = Some(error);
                            break 'outer;
                        }
                        Some(PiProcessEvent::Eof) | None => {
                            let status = process.try_exit().ok().flatten().map(|status| status.to_string()).unwrap_or_else(|| "unknown status".into());
                            let stderr = process.stderr_tail();
                            fatal = Some(if stderr.trim().is_empty() {
                                format!("Pi RPC exited ({status})")
                            } else {
                                format!("Pi RPC exited ({status}): {}", stderr.trim())
                            });
                            break 'outer;
                        }
                    }
                }
                request = request_rx.recv() => {
                    let Some(request) = request else {
                        fatal = Some("Pi adapter request channel closed".into());
                        break 'outer;
                    };
                    if let Err(error) = route_output(adapter.request(request), &rpc, &mut pending_inbound).await {
                        fatal = Some(error);
                        break 'outer;
                    }
                    first_inbound = pending_inbound.pop_front();
                }
                event = events.next_event() => {
                    match event {
                        Some(Ok(event)) => {
                            pending_event = Some(event);
                            scheduler.request(DirtyReason::Interactive, Instant::now());
                        }
                        Some(Err(error)) => {
                            fatal = Some(format!("terminal event stream failed: {error}"));
                            break 'outer;
                        }
                        None => {
                            fatal = Some("terminal event stream closed".into());
                            break 'outer;
                        }
                    }
                }
                _ = wait_for_deadline(frame_deadline) => {}
                _ = wait_for_deadline(notice_deadline) => {
                    let now = runtime_ports.now();
                    let expired = {
                        let mut state = state_r.lock().unwrap();
                        let duration = state.config.copy_toast_secs;
                        state.interaction.notice.expire(duration, now)
                    };
                    if expired {
                        scheduler.request(DirtyReason::Interactive, now);
                    }
                }
                _ = wait_for_deadline(animation_deadline) => {
                    let now = Instant::now();
                    let mut state = state_r.lock().unwrap();
                    let spinner_due = spinner_deadline.is_some_and(|deadline| deadline <= now);
                    let reveal_due = state.reveal_deadline().is_some_and(|deadline| deadline <= now);
                    let mut redraw = false;
                    if spinner_due {
                        redraw |= tick_spinners(&mut state, now);
                        spinner_deadline = animation_active(&state, now)
                            .then(|| now + animation_interval(state.config.spinner_frame_ms));
                    }
                    if reveal_due {
                        redraw |= state.tick_reveals(now);
                    }
                    if redraw {
                        scheduler.request(DirtyReason::Animation, now);
                    }
                }
            }
        }

        if let Some(first) = first_inbound {
            let _batch_zone = e_tui::tracy_zone!("inbound batch");
            let batch_started = Instant::now();
            let mut next = Some(first);
            let mut count = 0usize;
            while let Some(event) = next.take() {
                count += 1;
                let is_snapshot = matches!(
                    &event,
                    e_tui::AgentEvent::Timeline(e_tui::agent::TimelineEvent::Snapshot { .. })
                );
                // Streaming text/reasoning deltas render frame-by-frame so the
                // assistant output (and Thinking reasoning) appears
                // incrementally instead of being swallowed by the inbound
                // batch and jumping in whole chunks.
                let streaming_delta = is_streaming_delta(&event);
                if is_snapshot {
                    phases.mark("snapshot received");
                }
                let update_started = Instant::now();
                let mut interaction = {
                    let mut app = state_r.lock().unwrap();
                    std::mem::take(&mut app.interaction)
                };
                let mut ui = RuntimeUiState {
                    scroll: &mut interaction.scroll,
                    input: &mut interaction.input,
                    input_page: &mut interaction.input_page,
                    approval: &mut interaction.approval,
                    question: &mut interaction.question,
                    queue: &mut interaction.queue,
                };
                let effects = RuntimeController::apply_agent(event, &state_r, &mut ui);
                state_r.lock().unwrap().interaction = interaction;
                let mut agent = PiAgentPort {
                    requests: &request_tx,
                };
                let execution =
                    execute_ui_actions(effects, &mut agent, &mut scheduler, &mut runtime_ports)
                        .await;
                if let Some(reason) = execution.fatal {
                    fatal = Some(reason);
                    break 'outer;
                }
                if execution.quit {
                    break 'outer;
                }
                pending_update_elapsed += update_started.elapsed();
                if is_snapshot {
                    phases.mark("snapshot applied");
                }
                if streaming_delta {
                    // Hand the render loop back after every delta so each
                    // streamed increment becomes its own visible frame.
                    break;
                }
                if !inbound_budget_remaining(count, batch_started.elapsed()) {
                    break;
                }
                next = pending_inbound.pop_front();
            }
            scheduler.request(DirtyReason::Content, Instant::now());
        }

        // ---- directly-woken terminal event ----
        if let Some(event) = pending_event {
            let focus = {
                let state = state_r.lock().unwrap();
                let drafting = state.is_new_conversation();
                TerminalFocus {
                    help_visible: state.interaction.help_visible,
                    input_page_open: state.interaction.input_page.is_some(),
                    approval_open: !drafting && state.interaction.approval.is_some(),
                    reading_view_open: state.reading.is_some(),
                }
            };
            let route = route_terminal_event(event, focus, &config.key_mapping);
            let terminal_size = terminal.size();
            let terminal_height = terminal_size.as_ref().map(|s| s.height).unwrap_or(40);
            let terminal_width = terminal_size.as_ref().map(|s| s.width).unwrap_or(120);
            let mut interaction = {
                let mut app = state_r.lock().unwrap();
                std::mem::take(&mut app.interaction)
            };
            let effects = RuntimeController::apply_terminal_route(
                route,
                TerminalSize {
                    width: terminal_width,
                    height: terminal_height,
                },
                runtime_ports.now(),
                &state_r,
                &committed_selection_frame,
                &mut TerminalUiState {
                    scroll: &mut interaction.scroll,
                    input: &mut interaction.input,
                    input_page: &mut interaction.input_page,
                    help_visible: &mut interaction.help_visible,
                    notice: &mut interaction.notice,
                    mouse_selection: &mut interaction.mouse_selection,
                    pane_resize: &mut interaction.pane_resize,
                    approval: &mut interaction.approval,
                    question: &mut interaction.question,
                    queue: &mut interaction.queue,
                    config: &mut config,
                    themes: &mut themes,
                    theme: &mut theme,
                },
            );
            state_r.lock().unwrap().interaction = interaction;
            let mut agent = PiAgentPort {
                requests: &request_tx,
            };
            let execution =
                execute_ui_actions(effects, &mut agent, &mut scheduler, &mut runtime_ports).await;
            for result in execution.completed {
                match result {
                    EffectResult::ConfigReloaded {
                        config: loaded,
                        themes: loaded_themes,
                    } => {
                        let mut interaction = {
                            let mut app = state_r.lock().unwrap();
                            std::mem::take(&mut app.interaction)
                        };
                        RuntimeController::apply_reloaded_config(
                            *loaded,
                            loaded_themes,
                            &state_r,
                            &mut TerminalUiState {
                                scroll: &mut interaction.scroll,
                                input: &mut interaction.input,
                                input_page: &mut interaction.input_page,
                                help_visible: &mut interaction.help_visible,
                                notice: &mut interaction.notice,
                                mouse_selection: &mut interaction.mouse_selection,
                                pane_resize: &mut interaction.pane_resize,
                                approval: &mut interaction.approval,
                                question: &mut interaction.question,
                                queue: &mut interaction.queue,
                                config: &mut config,
                                themes: &mut themes,
                                theme: &mut theme,
                            },
                        );
                        state_r.lock().unwrap().interaction = interaction;
                    }
                    other => {
                        let now = runtime_ports.now();
                        if RuntimeController::apply_effect_result(other, &state_r, now) {
                            scheduler.request(DirtyReason::Content, now);
                        }
                    }
                }
            }
            if let Some(reason) = execution.fatal {
                fatal = Some(reason);
                break 'outer;
            }
            if execution.quit {
                break 'outer;
            }
        }

        // Process admitted input before claiming candidates so Escape can cancel them.
        let queued_effects = RuntimeController::dispatch_next_queued(&state_r);
        if !queued_effects.is_empty() {
            let mut agent = PiAgentPort {
                requests: &request_tx,
            };
            let execution = execute_ui_actions(
                queued_effects,
                &mut agent,
                &mut scheduler,
                &mut runtime_ports,
            )
            .await;
            if let Some(reason) = execution.fatal {
                fatal = Some(reason);
                break 'outer;
            }
            if execution.quit {
                break 'outer;
            }
            scheduler.request(DirtyReason::Content, Instant::now());
        }

        // Start the spinner clock only while a running/settling indicator
        // exists. Reveal lanes contribute their own exact deadlines at the
        // next select turn; a fully idle client has no periodic wakeup.
        if spinner_deadline.is_none() {
            let now = Instant::now();
            let state = state_r.lock().unwrap();
            if animation_active(&state, now) {
                spinner_deadline = Some(now);
            }
        }

        let now = Instant::now();
        if let Some(requested_at) = scheduler.take_due(now) {
            let mut interaction = {
                let mut app = state_r.lock().unwrap();
                std::mem::take(&mut app.interaction)
            };
            let mut state = state_r.lock().unwrap();
            let notice_now = runtime_ports.now();
            let notice = interaction
                .notice
                .visible_text(state.config.copy_toast_secs, notice_now);
            let first_frame = !first_draw_done;
            let _first_zone = if first_frame {
                e_tui::tracy_zone!("first frame")
            } else {
                None
            };
            let mut candidate_selection_frame = None;
            let transaction = terminal.draw(|frame| {
                let output = e_tui::ui::render_with_cursor_and_selection(
                    frame,
                    &mut state,
                    &interaction.input,
                    &mut interaction.scroll,
                    &theme,
                    e_tui::ui::RenderOverlays {
                        help_visible: interaction.help_visible,
                        toast: notice,
                        input_page: interaction.input_page.as_mut(),
                        settings: None,
                        login: None,
                        approval: interaction.approval.as_ref(),
                        queue: interaction.queue.entries(),
                        pane_resize: interaction.pane_resize,
                    },
                    &interaction.mouse_selection,
                    &committed_selection_frame,
                );
                candidate_selection_frame = Some(output.selection_frame);
                output.cursor
            });
            let cache_work = state.render.transcript_cache.take_work_stats();
            let preview_work = state.preview.take_work_stats();
            drop(state);
            state_r.lock().unwrap().interaction = interaction;
            let transaction = transaction?;
            let mut candidate_selection_frame =
                candidate_selection_frame.expect("render always produces selection geometry");
            let geometry_changed =
                !candidate_selection_frame.same_geometry(&committed_selection_frame);
            let epoch = if geometry_changed {
                committed_selection_frame.epoch().wrapping_add(1).max(1)
            } else {
                committed_selection_frame.epoch().max(1)
            };
            candidate_selection_frame.set_epoch(epoch);
            committed_selection_frame = candidate_selection_frame;
            if geometry_changed {
                state_r.lock().unwrap().interaction.mouse_selection.clear();
            }
            scheduler.complete(Instant::now());
            let io = transaction.io;
            let report = frame_metrics.record(FrameSample {
                scheduler_delay: now.saturating_duration_since(requested_at),
                update: pending_update_elapsed,
                render: transaction.render,
                draw: Duration::from_nanos(io.draw_ns),
                flush: Duration::from_nanos(io.flush_ns),
                total: transaction.total,
                changed_cells: io.changed_cells,
                emitted_bytes: io.emitted_bytes,
                cache_rebuilds: cache_work.rebuilds,
                cache_patches: cache_work.patches,
                materialized_rows: cache_work.materialized_rows,
                preview_rebuilds: preview_work.rebuilds,
                preview_patches: preview_work.patches,
                preview_materialized_rows: preview_work.materialized_rows,
            });
            pending_update_elapsed = Duration::ZERO;
            if let Some(report) = report {
                eprintln!("{report}");
            }
            if first_frame {
                drop(_first_zone);
                first_draw_done = true;
                phases.mark("first frame");
            }
        }
    }

    process.shutdown().await;
    terminal.restore_terminal().ok();
    if let Some(reason) = fatal {
        bail!("{reason}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args<'a>(items: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
        items.iter().map(|item| (*item).to_owned())
    }

    #[test]
    fn cli_defaults_to_native_trust_and_current_directory() {
        let cli = parse_cli_from(args(&[])).unwrap();
        assert_eq!(cli.launch.trust, ProjectTrust::Native);
        assert!(cli.launch.session.is_none());
    }

    #[test]
    fn cli_accepts_explicit_session_cwd_and_trust() {
        let cli = parse_cli_from(args(&[
            "--cwd",
            "project",
            "--session",
            "one.jsonl",
            "--approve",
        ]))
        .unwrap();
        assert_eq!(cli.launch.cwd, PathBuf::from("project"));
        assert_eq!(cli.launch.session.as_deref(), Some("one.jsonl"));
        assert_eq!(cli.launch.trust, ProjectTrust::Approve);
    }
}
