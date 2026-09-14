//! Pi-native authentication helper and companion-extension control plane.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::Duration,
};

use e_tui::{
    agent::{
        AuthMethod, AuthNotice, AuthNoticeKind, AuthOutcomeKind, AuthPrompt, AuthPromptKind,
        AuthPromptOption, AuthProvider, CatalogEvent, InteractionEvent,
    },
    AgentEvent, AgentRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
};

use crate::{
    adapter::AUTH_STATUS_KEY,
    framing::JsonlDecoder,
    protocol::{extension_ui_request, RpcRecord},
};

const MAX_RECORD_BYTES: usize = 256 * 1024;
const CHANNEL_CAPACITY: usize = 64;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

pub struct AuthAssets {
    _directory: tempfile::TempDir,
    companion: PathBuf,
    helper: PathBuf,
}

impl AuthAssets {
    pub fn materialize() -> anyhow::Result<Self> {
        let directory = tempfile::tempdir()?;
        let companion = directory.path().join("pie-auth-companion.mjs");
        let helper = directory.path().join("pie-auth-helper.mjs");
        std::fs::write(&companion, include_bytes!("auth_companion.mjs"))?;
        std::fs::write(&helper, include_bytes!("auth_helper.mjs"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&companion, std::fs::Permissions::from_mode(0o600))?;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            _directory: directory,
            companion,
            helper,
        })
    }

    pub fn companion(&self) -> &Path {
        &self.companion
    }

    fn helper(&self) -> &Path {
        &self.helper
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthContext {
    protocol: u64,
    package_root: PathBuf,
    agent_dir: PathBuf,
    node_path: PathBuf,
    cwd: PathBuf,
    session_id: String,
    project_trusted: bool,
    #[allow(dead_code)]
    pi_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthControl {
    Context {
        #[serde(flatten)]
        context: AuthContext,
    },
    Refresh {
        #[serde(rename = "flowId")]
        flow_id: String,
        success: bool,
        error: Option<String>,
        remote_refreshed: bool,
        remote_warning: Option<String>,
        #[allow(dead_code)]
        protocol: u64,
    },
}

pub fn control_from_record(record: &RpcRecord) -> Option<Result<AuthControl, String>> {
    if record.kind != "extension_ui_request" {
        return None;
    }
    let request = match extension_ui_request(record) {
        Ok(request) => request,
        Err(error) => return Some(Err(format!("invalid Pi authentication control: {error}"))),
    };
    if request.method != "setStatus" || request.status_key.as_deref() != Some(AUTH_STATUS_KEY) {
        return None;
    }
    let text = request.status_text.unwrap_or_default();
    if text.len() > MAX_RECORD_BYTES {
        return Some(Err("Pi authentication control exceeds 256 KiB".into()));
    }
    Some(
        serde_json::from_str(&text)
            .map_err(|error| format!("invalid Pi authentication companion response: {error}")),
    )
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum AuthAction {
    Event(AgentEvent),
    RefreshRuntime { flow_id: String, provider: String },
    Ignore,
    Deadline,
    Fatal(String),
}

#[derive(Debug, Clone)]
struct PendingOutcome {
    flow_id: String,
    outcome: AuthOutcomeKind,
    message: String,
    synchronized: bool,
}

pub struct AuthManager {
    assets: AuthAssets,
    context: Option<AuthContext>,
    helper: Option<AuthProcess>,
    queued: VecDeque<AuthAction>,
    last_catalog: Option<(Option<String>, bool)>,
    pending_catalog: Option<(String, Option<String>, bool)>,
    catalog_deadline: Option<tokio::time::Instant>,
    active_flow: Option<String>,
    pending_outcome: Option<PendingOutcome>,
    refresh_deadline: Option<tokio::time::Instant>,
    next_id: u64,
}

impl AuthManager {
    pub fn new(assets: AuthAssets) -> Self {
        Self {
            assets,
            context: None,
            helper: None,
            queued: VecDeque::new(),
            last_catalog: None,
            pending_catalog: None,
            catalog_deadline: None,
            active_flow: None,
            pending_outcome: None,
            refresh_deadline: None,
            next_id: 1,
        }
    }

    pub async fn configure(&mut self, context: AuthContext) -> Result<(), String> {
        if context.protocol != 1 {
            return Err(format!(
                "unsupported Pi authentication companion protocol {}",
                context.protocol
            ));
        }
        for (label, path) in [
            ("Pi package", &context.package_root),
            ("Pi agent directory", &context.agent_dir),
            ("Node executable", &context.node_path),
            ("session cwd", &context.cwd),
        ] {
            if !path.is_absolute() {
                return Err(format!("{label} reported a non-absolute path"));
            }
        }
        if !context.package_root.join("dist").join("index.js").is_file() {
            return Err("selected Pi runtime does not expose its public SDK entry point".into());
        }
        if self.context.as_ref() == Some(&context) && self.helper.is_some() {
            return Ok(());
        }
        let session_changed = self
            .context
            .as_ref()
            .is_some_and(|previous| previous.session_id != context.session_id);
        if let Some(helper) = self.helper.as_mut() {
            helper.shutdown().await;
        }
        if session_changed {
            self.queued.clear();
            if let Some(flow_id) = self.active_flow.take() {
                self.queued.push_back(AuthAction::Event(finished(
                    &flow_id,
                    AuthOutcomeKind::Unknown,
                    "Authentication was invalidated by a session change; the credential store is unchanged",
                )));
            }
            self.last_catalog = None;
            self.pending_catalog = None;
            self.catalog_deadline = None;
        } else if let Some(flow_id) = self.active_flow.take() {
            self.queued.push_back(AuthAction::Event(finished(
                &flow_id,
                AuthOutcomeKind::Unknown,
                "Authentication context changed; reconcile provider status before retrying",
            )));
        }
        self.pending_outcome = None;
        self.refresh_deadline = None;
        self.helper = Some(
            AuthProcess::spawn(&context, self.assets.helper())
                .await
                .map_err(|error| format!("cannot start Pi authentication helper: {error}"))?,
        );
        self.context = Some(context);
        if let Some((provider_ref, logout)) = self.last_catalog.clone() {
            let request_id = format!("catalog-{}", self.next());
            self.pending_catalog = Some((request_id.clone(), provider_ref.clone(), logout));
            self.catalog_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
            let _ = self.send(json!({
                "type": "catalog",
                "requestId": request_id,
                "providerRef": provider_ref,
                "logout": logout,
            }));
        }
        Ok(())
    }

    pub fn request(&mut self, request: AgentRequest) -> Vec<AuthAction> {
        match request {
            AgentRequest::AuthGet {
                provider_ref,
                logout,
            } => {
                self.last_catalog = Some((provider_ref.clone(), logout));
                let request_id = format!("catalog-{}", self.next());
                self.pending_catalog = Some((request_id.clone(), provider_ref.clone(), logout));
                self.catalog_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
                if self.send(json!({
                    "type": "catalog",
                    "requestId": request_id,
                    "providerRef": provider_ref,
                    "logout": logout,
                })) {
                    Vec::new()
                } else {
                    self.helper = None;
                    self.pending_catalog = None;
                    self.catalog_deadline = None;
                    vec![AuthAction::Event(catalog_error(
                        provider_ref,
                        logout,
                        "Pi native authentication is unavailable; restart pie or use native pi /login",
                    ))]
                }
            }
            AgentRequest::AuthStart {
                provider,
                method,
                logout,
            } => {
                if self.active_flow.is_some() {
                    return vec![
                        AuthAction::Event(started("busy")),
                        AuthAction::Event(finished(
                            "busy",
                            AuthOutcomeKind::Failed,
                            "Another authentication operation is already active",
                        )),
                    ];
                }
                let flow_id = format!("auth-{}", self.next());
                if !self.send(json!({
                    "type": "start",
                    "flowId": flow_id,
                    "provider": provider,
                    "method": method,
                    "logout": logout,
                })) {
                    self.helper = None;
                    return vec![
                        AuthAction::Event(started(&flow_id)),
                        AuthAction::Event(finished(
                            &flow_id,
                            AuthOutcomeKind::Failed,
                            "Pi native authentication is unavailable",
                        )),
                    ];
                }
                self.active_flow = Some(flow_id.clone());
                vec![AuthAction::Event(started(&flow_id))]
            }
            AgentRequest::AuthReply {
                flow_id,
                prompt_id,
                value,
            } => {
                if self.active_flow.as_deref() != Some(flow_id.as_str()) {
                    return Vec::new();
                }
                let sent = self.send(json!({
                    "type": "reply",
                    "flowId": flow_id,
                    "promptId": prompt_id,
                    "value": value,
                }));
                if sent {
                    Vec::new()
                } else {
                    self.helper = None;
                    self.active_flow = None;
                    vec![AuthAction::Event(finished(
                        &flow_id,
                        AuthOutcomeKind::Unknown,
                        "Authentication helper disconnected before acknowledging the reply",
                    ))]
                }
            }
            AgentRequest::AuthOpenUrl { flow_id, url } => {
                if self.active_flow.as_deref() != Some(flow_id.as_str()) || !safe_url(&url) {
                    return Vec::new();
                }
                match open_url(&url) {
                    Ok(()) => Vec::new(),
                    Err(message) => vec![AuthAction::Event(AgentEvent::Interaction(
                        InteractionEvent::Error {
                            code: "pi-authentication-url".into(),
                            message,
                        },
                    ))],
                }
            }
            AgentRequest::AuthCancel => {
                let flow_id = self.active_flow.clone();
                if self.send(json!({ "type": "cancel" })) {
                    Vec::new()
                } else if let Some(flow_id) = flow_id {
                    self.helper = None;
                    self.active_flow = None;
                    vec![AuthAction::Event(finished(
                        &flow_id,
                        AuthOutcomeKind::Unknown,
                        "Authentication helper disconnected before cancellation was acknowledged",
                    ))]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    pub async fn recv(&mut self) -> Option<AuthAction> {
        if let Some(action) = self.queued.pop_front() {
            return Some(action);
        }
        let deadline = [self.catalog_deadline, self.refresh_deadline]
            .into_iter()
            .flatten()
            .min();
        let event = match (self.helper.as_mut(), deadline) {
            (Some(helper), Some(deadline)) => tokio::select! {
                event = helper.recv() => event,
                _ = tokio::time::sleep_until(deadline) => return Some(AuthAction::Deadline),
            },
            (Some(helper), None) => helper.recv().await,
            (None, Some(deadline)) => {
                tokio::time::sleep_until(deadline).await;
                return Some(AuthAction::Deadline);
            }
            (None, None) => std::future::pending().await,
        }?;
        Some(match event {
            AuthProcessEvent::Record(value) => self.record(value),
            AuthProcessEvent::Fatal(error) => {
                let uncertain = self.active_flow.take();
                self.pending_outcome = None;
                self.refresh_deadline = None;
                self.helper = None;
                if let Some(flow_id) = uncertain {
                    AuthAction::Event(finished(
                        &flow_id,
                        AuthOutcomeKind::Unknown,
                        "Authentication helper disconnected; reconcile provider status before retrying",
                    ))
                } else {
                    AuthAction::Fatal(error)
                }
            }
            AuthProcessEvent::Eof => {
                let flow_id = self.active_flow.take();
                self.pending_outcome = None;
                self.refresh_deadline = None;
                self.helper = None;
                if let Some(flow_id) = flow_id {
                    AuthAction::Event(finished(
                        &flow_id,
                        AuthOutcomeKind::Unknown,
                        "Authentication helper exited; reconcile provider status before retrying",
                    ))
                } else {
                    AuthAction::Fatal("Pi authentication helper exited".into())
                }
            }
        })
    }

    pub fn control(&mut self, control: AuthControl) -> Vec<AuthAction> {
        match control {
            AuthControl::Context { .. } => Vec::new(),
            AuthControl::Refresh {
                flow_id,
                success,
                error,
                remote_refreshed,
                remote_warning,
                ..
            } => {
                let Some(pending) = self
                    .pending_outcome
                    .take()
                    .filter(|pending| pending.flow_id == flow_id)
                else {
                    return Vec::new();
                };
                self.active_flow = None;
                self.refresh_deadline = None;
                let (outcome, message) = if success {
                    let mut warnings = Vec::new();
                    if !pending.synchronized {
                        warnings
                            .push("helper snapshot synchronization reported a warning".to_owned());
                    }
                    if remote_refreshed {
                        warnings.push("remote model catalog refreshed".to_owned());
                    } else if let Some(warning) = remote_warning {
                        warnings.push(warning);
                    }
                    let warning = if warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", warnings.join("; "))
                    };
                    (pending.outcome, format!("{}{warning}", pending.message))
                } else {
                    (
                        AuthOutcomeKind::Failed,
                        format!(
                            "Credential changed, but the running Pi model registry could not refresh: {}; restart pie to retry synchronization without signing in again",
                            error.unwrap_or_else(|| "unknown refresh error".into())
                        ),
                    )
                };
                vec![AuthAction::Event(finished(&flow_id, outcome, &message))]
            }
        }
    }

    pub fn expire_deadlines(&mut self) -> Vec<AuthAction> {
        let now = tokio::time::Instant::now();
        let mut actions = Vec::new();
        if self
            .catalog_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.catalog_deadline = None;
            if let Some((_, provider_ref, logout)) = self.pending_catalog.take() {
                actions.push(AuthAction::Event(catalog_error(
                    provider_ref,
                    logout,
                    "Pi authentication provider discovery timed out",
                )));
            }
        }
        if self
            .refresh_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.refresh_deadline = None;
            if let Some(pending) = self.pending_outcome.take() {
                self.active_flow = None;
                actions.push(AuthAction::Event(finished(
                    &pending.flow_id,
                    AuthOutcomeKind::Failed,
                    "Credential changed, but the running Pi model registry refresh timed out; restart pie to retry synchronization without signing in again",
                )));
            }
        }
        actions
    }

    pub async fn shutdown(&mut self) {
        if let Some(helper) = self.helper.as_mut() {
            helper.shutdown().await;
        }
    }

    fn record(&mut self, value: Value) -> AuthAction {
        match value.get("type").and_then(Value::as_str) {
            Some("ready") if value.get("protocol").and_then(Value::as_u64) == Some(1) => {
                AuthAction::Event(AgentEvent::Interaction(InteractionEvent::Heartbeat))
            }
            Some("ready") => AuthAction::Fatal("unsupported authentication helper protocol".into()),
            Some("catalog") => match serde_json::from_value::<CatalogWire>(value) {
                Ok(wire)
                    if self
                        .pending_catalog
                        .as_ref()
                        .is_some_and(|(request_id, _, _)| request_id == &wire.request_id) =>
                {
                    self.pending_catalog = None;
                    self.catalog_deadline = None;
                    AuthAction::Event(AgentEvent::Catalog(CatalogEvent::Authentication {
                        providers: wire.providers.into_iter().map(Into::into).collect(),
                        provider_ref: wire.provider_ref,
                        logout: wire.logout,
                        error: wire.error,
                    }))
                }
                Ok(_) => AuthAction::Ignore,
                Err(error) => AuthAction::Fatal(format!("invalid authentication catalog: {error}")),
            },
            Some("prompt") => match serde_json::from_value::<PromptWire>(value) {
                Ok(wire) if self.active_flow.as_deref() == Some(wire.flow_id.as_str()) => {
                    AuthAction::Event(AgentEvent::Interaction(InteractionEvent::AuthPrompt(
                        wire.into(),
                    )))
                }
                Ok(_) => AuthAction::Ignore,
                Err(error) => AuthAction::Fatal(format!("invalid authentication prompt: {error}")),
            },
            Some("prompt_withdrawn") => {
                let flow_id = string_field(&value, "flowId");
                if self.active_flow.as_deref() != Some(flow_id.as_str()) {
                    return AuthAction::Ignore;
                }
                let prompt_id = string_field(&value, "promptId");
                AuthAction::Event(AgentEvent::Interaction(
                    InteractionEvent::AuthPromptWithdrawn { flow_id, prompt_id },
                ))
            }
            Some("notice") => match serde_json::from_value::<NoticeWire>(value) {
                Ok(wire) if self.active_flow.as_deref() == Some(wire.flow_id.as_str()) => {
                    AuthAction::Event(AgentEvent::Interaction(InteractionEvent::AuthNotice(
                        wire.into(),
                    )))
                }
                Ok(_) => AuthAction::Ignore,
                Err(error) => AuthAction::Fatal(format!("invalid authentication notice: {error}")),
            },
            Some("outcome") => match serde_json::from_value::<OutcomeWire>(value) {
                Ok(wire) if self.active_flow.as_deref() != Some(wire.flow_id.as_str()) => {
                    AuthAction::Ignore
                }
                Ok(wire) if wire.committed => {
                    let flow_id = wire.flow_id.clone();
                    let provider = wire.provider.clone();
                    let native_outcome = AuthOutcomeKind::from(wire.outcome);
                    let message = match native_outcome {
                        AuthOutcomeKind::Succeeded => wire.message,
                        AuthOutcomeKind::Cancelled => {
                            "Credential changed before cancellation completed".into()
                        }
                        AuthOutcomeKind::Failed | AuthOutcomeKind::Unknown => {
                            "Credential changed".into()
                        }
                    };
                    self.pending_outcome = Some(PendingOutcome {
                        flow_id: flow_id.clone(),
                        outcome: AuthOutcomeKind::Succeeded,
                        message,
                        synchronized: wire.synchronized,
                    });
                    self.refresh_deadline =
                        Some(tokio::time::Instant::now() + Duration::from_secs(15));
                    AuthAction::RefreshRuntime { flow_id, provider }
                }
                Ok(wire) => {
                    self.active_flow = None;
                    AuthAction::Event(finished(&wire.flow_id, wire.outcome.into(), &wire.message))
                }
                Err(error) => AuthAction::Fatal(format!("invalid authentication outcome: {error}")),
            },
            Some(kind) => AuthAction::Fatal(format!("unknown authentication helper event: {kind}")),
            None => AuthAction::Fatal("authentication helper event has no type".into()),
        }
    }

    fn send(&self, value: Value) -> bool {
        self.helper
            .as_ref()
            .is_some_and(|helper| helper.send(value).is_ok())
    }

    fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

fn string_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn catalog_error(provider_ref: Option<String>, logout: bool, message: &str) -> AgentEvent {
    AgentEvent::Catalog(CatalogEvent::Authentication {
        providers: Vec::new(),
        provider_ref,
        logout,
        error: Some(message.into()),
    })
}

fn started(flow_id: &str) -> AgentEvent {
    AgentEvent::Interaction(InteractionEvent::AuthStarted {
        flow_id: flow_id.into(),
    })
}

fn finished(flow_id: &str, outcome: AuthOutcomeKind, message: &str) -> AgentEvent {
    AgentEvent::Interaction(InteractionEvent::AuthFinished {
        flow_id: flow_id.into(),
        outcome,
        message: message.into(),
    })
}

fn safe_url(value: &str) -> bool {
    let remainder = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    remainder.is_some_and(|rest| !rest.is_empty() && !value.chars().any(char::is_control))
}

fn open_url(value: &str) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", value]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(value);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(value);
        command
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| "Could not open the authentication URL".to_owned())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogWire {
    request_id: String,
    provider_ref: Option<String>,
    logout: bool,
    providers: Vec<ProviderWire>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderWire {
    id: String,
    name: String,
    methods: Vec<MethodWire>,
    configured: bool,
    removable: bool,
    source: Option<String>,
}

#[derive(Deserialize)]
struct MethodWire {
    id: String,
    name: String,
    description: Option<String>,
}

impl From<ProviderWire> for AuthProvider {
    fn from(value: ProviderWire) -> Self {
        Self {
            id: value.id,
            name: value.name,
            methods: value
                .methods
                .into_iter()
                .map(|method| AuthMethod {
                    id: method.id,
                    name: method.name,
                    description: method.description,
                })
                .collect(),
            configured: value.configured,
            removable: value.removable,
            source: value.source,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptWire {
    flow_id: String,
    prompt_id: String,
    kind: PromptKindWire,
    message: String,
    placeholder: Option<String>,
    #[serde(default)]
    options: Vec<PromptOptionWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum PromptKindWire {
    Text,
    Secret,
    ManualCode,
    Select,
}

#[derive(Deserialize)]
struct PromptOptionWire {
    value: String,
    label: String,
    description: Option<String>,
}

impl From<PromptWire> for AuthPrompt {
    fn from(value: PromptWire) -> Self {
        Self {
            flow_id: value.flow_id,
            prompt_id: value.prompt_id,
            kind: match value.kind {
                PromptKindWire::Text => AuthPromptKind::Text,
                PromptKindWire::Secret => AuthPromptKind::Secret,
                PromptKindWire::ManualCode => AuthPromptKind::ManualCode,
                PromptKindWire::Select => AuthPromptKind::Select,
            },
            message: value.message,
            placeholder: value.placeholder,
            options: value
                .options
                .into_iter()
                .map(|option| AuthPromptOption {
                    value: option.value,
                    label: option.label,
                    description: option.description,
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NoticeWire {
    flow_id: String,
    kind: NoticeKindWire,
    message: String,
    url: Option<String>,
    code: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum NoticeKindWire {
    AuthorizationUrl,
    DeviceCode,
    Information,
    Progress,
}

impl From<NoticeWire> for AuthNotice {
    fn from(value: NoticeWire) -> Self {
        Self {
            flow_id: value.flow_id,
            kind: match value.kind {
                NoticeKindWire::AuthorizationUrl => AuthNoticeKind::AuthorizationUrl,
                NoticeKindWire::DeviceCode => AuthNoticeKind::DeviceCode,
                NoticeKindWire::Information => AuthNoticeKind::Information,
                NoticeKindWire::Progress => AuthNoticeKind::Progress,
            },
            message: value.message,
            url: value.url.filter(|url| safe_url(url)),
            code: value.code,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutcomeWire {
    flow_id: String,
    provider: String,
    committed: bool,
    synchronized: bool,
    outcome: OutcomeKindWire,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeKindWire {
    Succeeded,
    Cancelled,
    Failed,
    Unknown,
}

impl From<OutcomeKindWire> for AuthOutcomeKind {
    fn from(value: OutcomeKindWire) -> Self {
        match value {
            OutcomeKindWire::Succeeded => Self::Succeeded,
            OutcomeKindWire::Cancelled => Self::Cancelled,
            OutcomeKindWire::Failed => Self::Failed,
            OutcomeKindWire::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug)]
enum AuthProcessEvent {
    Record(Value),
    Fatal(String),
    Eof,
}

struct AuthProcess {
    child: Child,
    outbound: mpsc::Sender<Value>,
    inbound: mpsc::Receiver<AuthProcessEvent>,
}

impl AuthProcess {
    async fn spawn(context: &AuthContext, helper: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(&context.node_path)
            .arg(helper)
            .arg(&context.package_root)
            .arg(&context.agent_dir)
            .arg(&context.cwd)
            .arg(if context.project_trusted {
                "true"
            } else {
                "false"
            })
            .current_dir(&context.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("missing stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("missing stdout"))?;
        let (outbound, outbound_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (inbound_tx, inbound) = mpsc::channel(CHANNEL_CAPACITY);
        tokio::spawn(auth_write_loop(stdin, outbound_rx, inbound_tx.clone()));
        tokio::spawn(auth_read_loop(stdout, inbound_tx));
        Ok(Self {
            child,
            outbound,
            inbound,
        })
    }

    fn send(&self, value: Value) -> Result<(), ()> {
        self.outbound.try_send(value).map_err(|_| ())
    }

    async fn recv(&mut self) -> Option<AuthProcessEvent> {
        self.inbound.recv().await
    }

    async fn shutdown(&mut self) {
        let _ = self.send(json!({ "type": "shutdown" }));
        if tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.start_kill();
            let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await;
        }
    }
}

impl Drop for AuthProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.start_kill();
        }
    }
}

async fn auth_write_loop(
    mut stdin: tokio::process::ChildStdin,
    mut values: mpsc::Receiver<Value>,
    events: mpsc::Sender<AuthProcessEvent>,
) {
    while let Some(value) = values.recv().await {
        let mut wire = match serde_json::to_vec(&value) {
            Ok(wire) if wire.len() <= MAX_RECORD_BYTES => wire,
            Ok(_) => {
                let _ = events
                    .send(AuthProcessEvent::Fatal(
                        "authentication helper command exceeds 256 KiB".into(),
                    ))
                    .await;
                return;
            }
            Err(error) => {
                let _ = events
                    .send(AuthProcessEvent::Fatal(format!(
                        "cannot encode authentication helper command: {error}"
                    )))
                    .await;
                return;
            }
        };
        wire.push(b'\n');
        if let Err(error) = stdin.write_all(&wire).await {
            let _ = events
                .send(AuthProcessEvent::Fatal(format!(
                    "cannot write authentication helper stdin: {error}"
                )))
                .await;
            return;
        }
        if let Err(error) = stdin.flush().await {
            let _ = events
                .send(AuthProcessEvent::Fatal(format!(
                    "cannot flush authentication helper stdin: {error}"
                )))
                .await;
            return;
        }
    }
}

async fn auth_read_loop(
    mut stdout: tokio::process::ChildStdout,
    events: mpsc::Sender<AuthProcessEvent>,
) {
    let mut decoder = JsonlDecoder::new(MAX_RECORD_BYTES);
    let mut buffer = [0_u8; 8192];
    loop {
        match stdout.read(&mut buffer).await {
            Ok(0) => {
                if let Some(Err(error)) = decoder.finish() {
                    let _ = events
                        .send(AuthProcessEvent::Fatal(error.to_string()))
                        .await;
                } else {
                    let _ = events.send(AuthProcessEvent::Eof).await;
                }
                return;
            }
            Ok(read) => {
                for record in decoder.push(&buffer[..read]) {
                    match record {
                        Ok(value) => {
                            if events.send(AuthProcessEvent::Record(value)).await.is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = events
                                .send(AuthProcessEvent::Fatal(error.to_string()))
                                .await;
                            return;
                        }
                    }
                }
            }
            Err(error) => {
                let _ = events
                    .send(AuthProcessEvent::Fatal(format!(
                        "cannot read authentication helper stdout: {error}"
                    )))
                    .await;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> AuthManager {
        AuthManager::new(AuthAssets::materialize().unwrap())
    }

    #[test]
    fn companion_status_is_recognized_only_for_the_owned_key() {
        let record = RpcRecord::from_value(json!({
            "type": "extension_ui_request",
            "id": "context",
            "method": "setStatus",
            "statusKey": AUTH_STATUS_KEY,
            "statusText": serde_json::to_string(&json!({
                "kind": "context",
                "protocol": 1,
                "packageRoot": "C:/pi",
                "agentDir": "C:/agent",
                "nodePath": "C:/node.exe",
                "cwd": "C:/work",
                "sessionId": "session-1",
                "projectTrusted": true,
                "piVersion": "0.85.1"
            })).unwrap()
        }))
        .unwrap();
        assert!(matches!(
            control_from_record(&record),
            Some(Ok(AuthControl::Context { context })) if context.protocol == 1
        ));

        let unrelated = RpcRecord::from_value(json!({
            "type": "extension_ui_request",
            "id": "status",
            "method": "setStatus",
            "statusKey": "another-extension",
            "statusText": "{}"
        }))
        .unwrap();
        assert!(control_from_record(&unrelated).is_none());
    }

    #[test]
    fn helper_catalog_and_prompt_are_normalized_without_provider_dtos() {
        let mut manager = manager();
        manager.pending_catalog = Some(("catalog-1".into(), Some("custom".into()), false));
        let catalog = manager.record(json!({
            "type": "catalog",
            "requestId": "catalog-1",
            "providerRef": "custom",
            "logout": false,
            "providers": [{
                "id": "custom",
                "name": "Custom",
                "methods": [{"id": "oauth", "name": "Browser"}],
                "configured": false,
                "removable": false
            }]
        }));
        assert!(matches!(
            catalog,
            AuthAction::Event(AgentEvent::Catalog(CatalogEvent::Authentication {
                providers,
                provider_ref: Some(reference),
                ..
            })) if providers[0].methods[0].id == "oauth" && reference == "custom"
        ));

        manager.active_flow = Some("flow".into());
        let prompt = manager.record(json!({
            "type": "prompt",
            "flowId": "flow",
            "promptId": "prompt",
            "kind": "secret",
            "message": "Enter token",
            "options": []
        }));
        assert!(matches!(
            prompt,
            AuthAction::Event(AgentEvent::Interaction(InteractionEvent::AuthPrompt(
                AuthPrompt {
                    kind: AuthPromptKind::Secret,
                    ..
                }
            )))
        ));
    }

    #[test]
    fn helper_records_require_current_request_and_flow_ids() {
        let mut manager = manager();
        manager.pending_catalog = Some(("current-catalog".into(), None, false));
        assert!(matches!(
            manager.record(json!({
                "type": "catalog",
                "requestId": "stale-catalog",
                "providerRef": null,
                "logout": false,
                "providers": []
            })),
            AuthAction::Ignore
        ));
        assert!(manager.pending_catalog.is_some());

        manager.active_flow = Some("current-flow".into());
        assert!(matches!(
            manager.record(json!({
                "type": "prompt",
                "flowId": "stale-flow",
                "promptId": "prompt",
                "kind": "text",
                "message": "stale",
                "options": []
            })),
            AuthAction::Ignore
        ));
        assert!(matches!(
            manager.record(json!({
                "type": "outcome",
                "flowId": "stale-flow",
                "provider": "custom",
                "committed": true,
                "synchronized": true,
                "outcome": "succeeded",
                "message": "stale"
            })),
            AuthAction::Ignore
        ));
        assert!(manager.pending_outcome.is_none());
    }

    #[test]
    fn committed_outcome_waits_for_live_runtime_refresh() {
        let mut manager = manager();
        manager.active_flow = Some("flow".into());
        let action = manager.record(json!({
            "type": "outcome",
            "flowId": "flow",
            "provider": "custom",
            "committed": true,
            "synchronized": true,
            "outcome": "succeeded",
            "message": "Authentication completed"
        }));
        assert!(matches!(
            action,
            AuthAction::RefreshRuntime { flow_id, provider }
                if flow_id == "flow" && provider == "custom"
        ));
        let actions = manager.control(AuthControl::Refresh {
            flow_id: "flow".into(),
            success: true,
            error: None,
            remote_refreshed: true,
            remote_warning: None,
            protocol: 1,
        });
        assert!(matches!(
            actions.as_slice(),
            [AuthAction::Event(AgentEvent::Interaction(
                InteractionEvent::AuthFinished {
                    outcome: AuthOutcomeKind::Succeeded,
                    message,
                    ..
                }
            ))] if message.contains("remote model catalog refreshed")
        ));
        assert!(manager.active_flow.is_none());

        manager.active_flow = Some("flow-warning".into());
        let action = manager.record(json!({
            "type": "outcome",
            "flowId": "flow-warning",
            "provider": "custom",
            "committed": true,
            "synchronized": false,
            "outcome": "failed",
            "message": "must not surface arbitrary provider details"
        }));
        assert!(matches!(action, AuthAction::RefreshRuntime { .. }));
        let actions = manager.control(AuthControl::Refresh {
            flow_id: "flow-warning".into(),
            success: true,
            error: None,
            remote_refreshed: false,
            remote_warning: Some("remote model catalog refresh skipped in offline mode".into()),
            protocol: 1,
        });
        assert!(matches!(
            actions.as_slice(),
            [AuthAction::Event(AgentEvent::Interaction(
                InteractionEvent::AuthFinished {
                    outcome: AuthOutcomeKind::Succeeded,
                    message,
                    ..
                }
            ))] if message.contains("Credential changed")
                && message.contains("helper snapshot synchronization reported a warning")
                && !message.contains("arbitrary provider details")
        ));

        manager.active_flow = Some("flow-refresh-failure".into());
        assert!(matches!(
            manager.record(json!({
                "type": "outcome",
                "flowId": "flow-refresh-failure",
                "provider": "custom",
                "committed": true,
                "synchronized": true,
                "outcome": "succeeded",
                "message": "Authentication completed"
            })),
            AuthAction::RefreshRuntime { .. }
        ));
        let actions = manager.control(AuthControl::Refresh {
            flow_id: "flow-refresh-failure".into(),
            success: false,
            error: Some("Running Pi provider refresh failed".into()),
            remote_refreshed: false,
            remote_warning: None,
            protocol: 1,
        });
        assert!(matches!(
            actions.as_slice(),
            [AuthAction::Event(AgentEvent::Interaction(
                InteractionEvent::AuthFinished {
                    outcome: AuthOutcomeKind::Failed,
                    message,
                    ..
                }
            ))] if message.contains("restart pie to retry synchronization")
        ));
    }

    #[test]
    fn authentication_urls_are_restricted_to_safe_http_targets() {
        assert!(safe_url("https://example.test/callback"));
        assert!(safe_url("http://localhost:8484/"));
        assert!(!safe_url("javascript:alert(1)"));
        assert!(!safe_url("https://"));
        assert!(!safe_url("https://example.test/\nnext"));
    }

    #[test]
    fn debug_projection_names_requests_and_redacts_reply_payloads() {
        let request = AgentRequest::AuthReply {
            flow_id: "flow".into(),
            prompt_id: "prompt".into(),
            value: "must-not-appear".into(),
        };
        let rendered = format!("{request:?}");
        assert!(rendered.contains("AuthReply"));
        assert!(rendered.contains("flow"));
        assert!(!rendered.contains("must-not-appear"));

        let request = AgentRequest::LoginSetApiKey {
            provider: "openai".into(),
            value: "must-not-appear".into(),
        };
        let rendered = format!("{request:?}");
        assert!(rendered.contains("openai"));
        assert!(!rendered.contains("must-not-appear"));

        assert_eq!(format!("{:?}", AgentRequest::Ping), "Ping");
    }
}
