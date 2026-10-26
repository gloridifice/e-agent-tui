//! Independently scheduled Pi session I/O. Terminal input never waits for this loop.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use e_tui::{
    agent::{AuthOutcomeKind, CatalogEvent, InteractionEvent, SessionEvent, TimelineEvent},
    runtime::{
        sessions::{
            SessionEnvelope, SessionId, SideCreation, SidePreparationResult, SideSessionSeed,
        },
        RuntimeState,
    },
    AgentEvent, AgentRequest,
};
use tokio::sync::{mpsc, watch};

use crate::{
    adapter::{AdapterOutput, AUTH_REFRESH_COMMAND},
    auth::{control_from_record, AuthAction, AuthAssets, AuthControl, AuthManager},
    execution_history_store::HistoryRecorder,
    process::{PiLaunchOptions, PiProcessEvent},
    protocol::RpcCommand,
    session_runtime::PiSessionRuntime,
    side::PreparedSideFile,
    tips::Availability,
};

const CHANNEL_CAPACITY: usize = 256;

pub enum SessionWorkerEvent {
    Agent(Box<AgentEvent>),
    Availability(Availability),
    Ready,
    SidePrepared {
        creation: SideCreation,
        result: SidePreparationResult,
        artifact: Option<PreparedSideFile>,
    },
    Fatal(String),
}

pub struct SessionWorker {
    id: SessionId,
    requests: mpsc::Sender<AgentRequest>,
    preparations: mpsc::Sender<SideCreation>,
    retry_config: watch::Sender<bool>,
    cancellation: watch::Sender<Option<SideCreation>>,
    stop: watch::Sender<bool>,
    events: mpsc::Receiver<SessionEnvelope<SessionWorkerEvent>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SessionWorker {
    pub fn start(
        id: SessionId,
        launch: PiLaunchOptions,
        state: Arc<Mutex<RuntimeState>>,
        history: Arc<Mutex<HistoryRecorder>>,
        error_auto_retry: bool,
    ) -> Self {
        Self::start_seeded(id, launch, state, history, error_auto_retry, None)
    }

    pub fn start_seeded(
        id: SessionId,
        launch: PiLaunchOptions,
        state: Arc<Mutex<RuntimeState>>,
        history: Arc<Mutex<HistoryRecorder>>,
        error_auto_retry: bool,
        seed: Option<SideSessionSeed>,
    ) -> Self {
        let (requests, request_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (preparations, preparation_rx) = mpsc::channel(1);
        let (retry_config, retry_rx) = watch::channel(error_auto_retry);
        let (cancellation, cancel_rx) = watch::channel(None);
        let (stop, mut stop_rx) = watch::channel(false);
        let (event_tx, events) = mpsc::channel(CHANNEL_CAPACITY);
        let task = tokio::spawn(async move {
            let prepared = tokio::select! {
                biased;
                _ = stop_rx.wait_for(|stop| *stop) => return,
                prepared = tokio::time::timeout(Duration::from_secs(30), prepare(launch)) => prepared.unwrap_or_else(|_| Err("Pi process startup timed out".into())),
            };
            let result = match prepared {
                Ok((mut runtime, mut auth)) => {
                    let mut io = WorkerIo {
                        id,
                        events: &event_tx,
                        history,
                        reported_history_error: None,
                        pending: VecDeque::new(),
                    };
                    let result = tokio::select! {
                        biased;
                        _ = stop_rx.wait_for(|stop| *stop) => Ok(()),
                        result = async {
                            if let Some(seed) = seed {
                                tokio::time::timeout(Duration::from_secs(30), initialize_seed(&mut runtime, &mut auth, seed)).await
                                    .map_err(|_| "Side session initialization timed out".to_owned())??;
                            }
                            run(&mut runtime, &mut auth, state, &mut io, WorkerInbox {
                                requests: request_rx, preparations: preparation_rx, retry_config: retry_rx, cancellation: cancel_rx,
                            }).await
                        } => result,
                    };
                    auth.shutdown().await;
                    runtime.process.shutdown().await;
                    result
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                tokio::select! {
                    biased;
                    _ = stop_rx.wait_for(|stop| *stop) => {},
                    _ = event_tx.send(SessionEnvelope { session: id, payload: SessionWorkerEvent::Fatal(error) }) => {},
                }
            }
        });
        Self {
            id,
            requests,
            preparations,
            retry_config,
            cancellation,
            stop,
            events,
            task: Some(task),
        }
    }

    pub fn requests(&self) -> mpsc::Sender<AgentRequest> {
        self.requests.clone()
    }

    pub fn prepare_side(&self, creation: SideCreation) -> Result<(), String> {
        if creation.source() != self.id {
            return Err("The side source no longer belongs to this worker".into());
        }
        self.preparations
            .try_send(creation)
            .map_err(|_| "Side preparation is unavailable or already queued".into())
    }

    pub fn cancel_side(&self, creation: SideCreation) {
        self.cancellation.send_replace(Some(creation));
    }

    pub fn set_error_auto_retry(&self, value: bool) {
        self.retry_config.send_if_modified(|current| {
            if *current == value {
                false
            } else {
                *current = value;
                true
            }
        });
    }

    pub async fn recv(&mut self) -> Option<SessionEnvelope<SessionWorkerEvent>> {
        self.events.recv().await
    }

    pub fn request_shutdown(&self) {
        self.stop.send_replace(true);
    }

    pub async fn shutdown(&mut self) {
        self.request_shutdown();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for SessionWorker {
    fn drop(&mut self) {
        // The detached task retains its resources until asynchronous shutdown finishes.
        self.stop.send_replace(true);
    }
}

async fn prepare(mut launch: PiLaunchOptions) -> Result<(PiSessionRuntime, AuthManager), String> {
    let assets = tokio::task::spawn_blocking(AuthAssets::materialize)
        .await
        .map_err(|error| format!("Pi companion preparation failed: {error}"))?
        .map_err(|error| format!("Pi companion preparation failed: {error}"))?;
    launch
        .extension_paths
        .push(assets.companion().to_path_buf());
    let runtime = PiSessionRuntime::spawn(&launch)
        .await
        .map_err(|error| error.to_string())?;
    Ok((runtime, AuthManager::new(assets)))
}

async fn initialize_seed(
    runtime: &mut PiSessionRuntime,
    auth: &mut AuthManager,
    seed: SideSessionSeed,
) -> Result<(), String> {
    let mut commands = Vec::new();
    if seed.session_id.is_none() {
        commands.push((
            "side-init-new",
            RpcCommand::NewChildSession {
                id: "side-init-new".into(),
                parent_session: seed.parent_session_id,
            },
        ));
    }
    if let Some(model) = seed.model {
        commands.push((
            "side-init-model",
            RpcCommand::SetModel {
                id: Some("side-init-model".into()),
                provider: model.provider,
                model_id: model.model,
            },
        ));
        if let Some(level) = model.reasoning_effort {
            commands.push((
                "side-init-effort",
                RpcCommand::SetThinkingLevel {
                    id: Some("side-init-effort".into()),
                    level,
                },
            ));
        }
    }
    for (id, command) in commands {
        runtime
            .process
            .sender()
            .send(command)
            .await
            .map_err(|_| "Pi stdin closed during side initialization")?;
        loop {
            let record = match runtime.process.recv().await {
                Some(PiProcessEvent::Record(record)) => record,
                _ => return Err("Pi exited during side initialization".into()),
            };
            if record.kind == "response" && record.string("id") == Some(id) {
                let response =
                    crate::protocol::response(&record).map_err(|error| error.to_string())?;
                if !response.success
                    || response
                        .data
                        .as_ref()
                        .and_then(|data| data.get("cancelled"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                {
                    return Err(response
                        .error
                        .unwrap_or_else(|| "Side initialization was cancelled".into()));
                }
                break;
            }
            if let Some(Ok(AuthControl::Context { context })) = control_from_record(&record) {
                auth.configure(context).await?;
            }
        }
    }
    Ok(())
}

struct WorkerIo<'a> {
    id: SessionId,
    events: &'a mpsc::Sender<SessionEnvelope<SessionWorkerEvent>>,
    history: Arc<Mutex<HistoryRecorder>>,
    reported_history_error: Option<String>,
    pending: VecDeque<AgentEvent>,
}

impl WorkerIo<'_> {
    async fn publish(&self, payload: SessionWorkerEvent) -> Result<(), String> {
        self.events
            .send(SessionEnvelope {
                session: self.id,
                payload,
            })
            .await
            .map_err(|_| "Pi frontend event channel closed".into())
    }

    async fn output(
        &mut self,
        output: AdapterOutput,
        rpc: &mpsc::Sender<RpcCommand>,
    ) -> Result<(), String> {
        for command in output.commands {
            rpc.send(command)
                .await
                .map_err(|_| "Pi RPC stdin channel closed".to_owned())?;
        }
        for mut event in output.events {
            let attached = matches!(&event, AgentEvent::Session(SessionEvent::Attached(_)));
            let recorded = {
                let mut history = self.history.lock().unwrap();
                let result = history.observe(&event);
                history.enrich_resume(&mut event);
                result
            };
            self.pending.push_back(event);
            match recorded {
                Ok(()) if attached => self.reported_history_error = None,
                Err(error) if self.reported_history_error.as_ref() != Some(&error) => {
                    self.reported_history_error = Some(error.clone());
                    self.pending
                        .push_back(error_event("history-recording", error));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn error_event(code: &str, message: String) -> AgentEvent {
    AgentEvent::Interaction(InteractionEvent::Error {
        code: code.into(),
        message,
    })
}

pub async fn route_auth_action(
    action: AuthAction,
    rpc: &mpsc::Sender<RpcCommand>,
    pending: &mut VecDeque<AgentEvent>,
) -> Result<(), String> {
    match action {
        AuthAction::Event(event) => pending.push_back(event),
        AuthAction::RefreshRuntime { flow_id, provider } => {
            let payload = serde_json::to_string(
                &serde_json::json!({ "flowId": &flow_id, "provider": &provider }),
            )
            .map_err(|error| format!("cannot encode Pi authentication refresh: {error}"))?;
            rpc.send(RpcCommand::Prompt {
                id: Some(format!("auth-refresh-{flow_id}")),
                message: format!("/{AUTH_REFRESH_COMMAND} {payload}"),
                streaming_behavior: None,
            })
            .await
            .map_err(|_| "Pi RPC stdin channel closed".to_owned())?;
        }
        AuthAction::Ignore | AuthAction::Deadline => {}
        AuthAction::Fatal(message) => pending.push_back(error_event("pi-authentication", message)),
    }
    Ok(())
}

struct WorkerInbox {
    requests: mpsc::Receiver<AgentRequest>,
    preparations: mpsc::Receiver<SideCreation>,
    retry_config: watch::Receiver<bool>,
    cancellation: watch::Receiver<Option<SideCreation>>,
}

async fn run(
    runtime: &mut PiSessionRuntime,
    auth: &mut AuthManager,
    state: Arc<Mutex<RuntimeState>>,
    io: &mut WorkerIo<'_>,
    inbox: WorkerInbox,
) -> Result<(), String> {
    let WorkerInbox {
        mut requests,
        mut preparations,
        mut retry_config,
        mut cancellation,
    } = inbox;
    let rpc = runtime.process.sender();
    io.output(
        AdapterOutput {
            commands: runtime.adapter.startup_commands(),
            events: Vec::new(),
        },
        &rpc,
    )
    .await?;
    let mut last_availability = None;
    let (mut attached, mut history, mut commands, mut models, mut initialized) =
        (false, false, false, false, false);
    let mut creation = None;
    let mut materializing = None;
    let mut materializations = tokio::task::JoinSet::new();
    let initialization_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let retry = *retry_config.borrow_and_update();
        io.output(runtime.adapter.set_error_auto_retry(retry), &rpc)
            .await?;
        while let Some(event) = io.pending.pop_front() {
            attached |= matches!(&event, AgentEvent::Session(SessionEvent::Attached(_)));
            history |= matches!(&event, AgentEvent::Timeline(TimelineEvent::Snapshot { .. }));
            commands |= matches!(&event, AgentEvent::Catalog(CatalogEvent::Commands(_)));
            models |= matches!(&event, AgentEvent::Catalog(CatalogEvent::Models { .. }));
            io.publish(SessionWorkerEvent::Agent(Box::new(event)))
                .await?;
        }
        let availability = runtime
            .adapter
            .tip_availability(auth.available_for_session(runtime.adapter.native_session_id()));
        if last_availability.as_ref() != Some(&availability) {
            io.publish(SessionWorkerEvent::Availability(availability.clone()))
                .await?;
            last_availability = Some(availability);
        }
        if !initialized && attached && history && commands && models {
            initialized = true;
            io.publish(SessionWorkerEvent::Ready).await?;
        }
        let deadline = e_tui::reveal::earliest_deadline(
            e_tui::reveal::earliest_deadline(
                e_tui::reveal::earliest_deadline(
                    runtime.adapter.retry_deadline(),
                    runtime.adapter.mcp_deadline(),
                ),
                runtime.side.deadline(),
            ),
            (!initialized).then_some(initialization_deadline),
        );
        tokio::select! {
            record = runtime.process.recv() => {
                let record = match record {
                    Some(PiProcessEvent::Record(record)) => record,
                    Some(PiProcessEvent::Fatal(error)) => return Err(error),
                    Some(PiProcessEvent::Eof) | None => {
                        let status = runtime.process.try_exit().ok().flatten().map(|status| status.to_string()).unwrap_or_else(|| "unknown status".into());
                        let stderr = runtime.process.stderr_tail();
                        return Err(if stderr.trim().is_empty() { format!("Pi RPC exited ({status})") } else { format!("Pi RPC exited ({status}): {}", stderr.trim()) });
                    }
                };
                if let Some(result) = runtime.side.record(&record, runtime.adapter.native_session_id(), Instant::now()) {
                    if let Some(creation) = creation.take() {
                        if let SidePreparationResult::Ready(mut seed) = result {
                            let directory = runtime.side.take_destination();
                            let staging_root = auth.companion_directory().to_owned();
                            materializing = Some(creation);
                            materializations.spawn(async move {
                                let (result, artifact) = match tokio::time::timeout(Duration::from_secs(15), PreparedSideFile::materialize(&mut seed, directory, &staging_root)).await {
                                    Ok(Ok(artifact)) => (SidePreparationResult::Ready(seed), artifact),
                                    Ok(Err(error)) => (SidePreparationResult::Failed(error), None),
                                    Err(_) => (SidePreparationResult::Failed("Side materialization timed out".into()), None),
                                };
                                SessionWorkerEvent::SidePrepared { creation, result, artifact }
                            });
                        } else {
                            io.publish(SessionWorkerEvent::SidePrepared { creation, result, artifact: None }).await?;
                        }
                    }
                }
                if let Some(control) = control_from_record(&record) {
                    match control {
                        Ok(AuthControl::Context { context }) => {
                            let configured = tokio::time::timeout(Duration::from_secs(15), auth.configure(context)).await;
                            let error = match configured {
                                Ok(Ok(())) => None,
                                Ok(Err(error)) => Some(error),
                                Err(_) => Some("Pi authentication helper startup timed out".into()),
                            };
                            if let Some(error) = error { io.pending.push_back(error_event("pi-authentication", error)); }
                        }
                        Ok(control) => {
                            for action in auth.control(control) { route_auth_action(action, &rpc, &mut io.pending).await?; }
                            io.output(runtime.adapter.request(AgentRequest::ModelGet), &rpc).await?;
                        }
                        Err(error) => io.pending.push_back(error_event("pi-authentication", error)),
                    }
                }
                let source = runtime.adapter.native_session_id().to_owned();
                if !crate::side::is_control_record(&record) {
                    io.output(runtime.adapter.record(record), &rpc).await?;
                }
                if runtime.adapter.native_session_id() != source {
                    if let Some(command) = runtime.side.cancel_command() { let _ = rpc.send(command).await; }
                    if let Some(creation) = creation.take() { io.publish(SessionWorkerEvent::SidePrepared { creation, result: SidePreparationResult::Cancelled, artifact: None }).await?; }
                }
            }
            request = requests.recv() => {
                let Some(request) = request else { return Ok(()); };
                if matches!(&request, AgentRequest::AuthGet { .. } | AgentRequest::AuthStart { .. } | AgentRequest::AuthReply { .. } | AgentRequest::AuthOpenUrl { .. } | AgentRequest::AuthCancel) {
                    let blocked = matches!(&request, AgentRequest::AuthStart { .. }) && {
                        let state = state.lock().unwrap();
                        state.session.status != e_tui::SessionStatus::Idle || state.session.working
                    };
                    if blocked {
                        io.pending.push_back(AgentEvent::Interaction(InteractionEvent::AuthStarted { flow_id: "blocked".into() }));
                        io.pending.push_back(AgentEvent::Interaction(InteractionEvent::AuthFinished { flow_id: "blocked".into(), outcome: AuthOutcomeKind::Failed, message: "Wait for the current Pi turn to finish before changing credentials".into() }));
                    } else {
                        for action in auth.request(request) { route_auth_action(action, &rpc, &mut io.pending).await?; }
                    }
                } else { io.output(runtime.adapter.request(request), &rpc).await?; }
            }
            completed = materializations.join_next(), if !materializations.is_empty() => {
                if let Some(creation) = materializing.take() {
                    let event = match completed {
                        Some(Ok(event)) => event,
                        _ => SessionWorkerEvent::SidePrepared { creation, result: SidePreparationResult::Failed("Side materialization stopped".into()), artifact: None },
                    };
                    io.publish(event).await?;
                }
            }
            requested = preparations.recv(), if initialized && materializations.is_empty() => {
                let Some(requested) = requested else { return Ok(()); };
                if *cancellation.borrow() == Some(requested) { continue; }
                match runtime.prepare_side(requested.kind(), Instant::now()) {
                    Ok(command) => {
                        creation = Some(requested);
                        rpc.send(command).await.map_err(|_| "Pi RPC stdin channel closed".to_owned())?;
                    }
                    Err(message) => io.publish(SessionWorkerEvent::SidePrepared { creation: requested, result: SidePreparationResult::Failed(message), artifact: None }).await?,
                }
            }
            action = auth.recv() => {
                if let Some(action) = action {
                    route_auth_action(action, &rpc, &mut io.pending).await?;
                    for action in auth.expire_deadlines() { route_auth_action(action, &rpc, &mut io.pending).await?; }
                }
            }
            changed = cancellation.changed() => {
                if changed.is_err() { return Ok(()); }
                let cancelled = *cancellation.borrow_and_update();
                if creation.is_some() && cancelled == creation {
                    if let Some(command) = runtime.side.cancel_command() { let _ = rpc.send(command).await; }
                    if let Some(creation) = creation.take() { io.publish(SessionWorkerEvent::SidePrepared { creation, result: SidePreparationResult::Cancelled, artifact: None }).await?; }
                }
            }
            changed = retry_config.changed() => { if changed.is_err() { return Ok(()); } }
            _ = e_tui::runtime::wait_for_deadline(deadline) => {
                if !initialized && Instant::now() >= initialization_deadline {
                    return Err("Pi session initialization timed out before state, history, commands and models were ready".into());
                }
                io.output(runtime.adapter.tick_retry(Instant::now()), &rpc).await?;
                io.output(runtime.adapter.tick_mcp(Instant::now()), &rpc).await?;
                if runtime.side.deadline().is_some_and(|deadline| deadline <= Instant::now()) {
                    if let Some(command) = runtime.side.cancel_command() { let _ = rpc.send(command).await; }
                    if let Some(creation) = creation.take() { io.publish(SessionWorkerEvent::SidePrepared { creation, result: SidePreparationResult::Failed("Side preparation timed out".into()), artifact: None }).await?; }
                }
            }
        }
    }
}
