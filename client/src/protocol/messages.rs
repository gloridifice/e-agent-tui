//! Client/server frame DTOs.

use serde::{Deserialize, Serialize};

use super::HostEvent;

/// Client → bridge messages.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ClientMessage {
    /// Authenticate and optionally attach to a specific live session.
    Hello {
        token: String,
        /// A fresh process with no resume target opens a NEW session on the
        /// bridge (each process shows one session).
        #[serde(skip_serializing_if = "Option::is_none")]
        resume_session_id: Option<String>,
        /// The directory the TUI was launched from. The bridge opens new
        /// sessions in this workspace (falling back to the attached
        /// session's header cwd when absent).
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// The configured default agent-preset mode for the session created
        /// at startup (the bridge falls back to `standard` when stale).
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        /// Wire contract understood by this client. Older bridges ignore it;
        /// newer bridges reject only clients requiring a newer contract.
        protocol_version: u64,
    },
    /// Ordinary user message (enters the agent inbox).
    Input { text: String },
    /// Slash command line.
    Command { line: String },
    /// Interrupt the current turn.
    Interrupt,
    /// Switch the connection to another live session.
    Attach { session_id: String },
    /// Request the session list (for the `/resume` Input Page).
    ListSessions,
    /// Answer one pending approval.
    ApprovalAnswer { id: String, allow: bool },
    /// Submit answers for one pending user-question batch.
    AnswerQuestions {
        rpc_id: String,
        answers: Vec<QuestionAnswer>,
    },
    /// Cancel one pending user-question batch (the host resolves the tool
    /// call as cancelled).
    CancelQuestions { rpc_id: String },
    /// Request older history: surface events with seq < `before_seq`,
    /// newest first from the stored log (lazy scroll-back paging).
    History { before_seq: u64, limit: usize },
    /// Read the login page state (providers / proxies).
    LoginGet,
    /// Store one provider's API key (empty clears it; the value itself is
    /// never read back — only its configured/source/hint view).
    LoginSetApiKey { provider: String, value: String },
    /// Create a custom proxy provider route.
    LoginProxyCreate {
        base_url: String,
        api_key: String,
        protocol: String,
        model: String,
    },
    /// Remove one custom proxy provider route.
    LoginProxyDelete { id: String },
    /// Request the provider/model catalog (for the `/model` picker).
    ModelGet,
    /// Select the provider/model for the attached session.
    ModelSet { provider: String, model: String },
    /// Keepalive.
    Ping,
}

/// One question in an ask_user_question batch (as received from the host).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionItem {
    pub id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    #[serde(default)]
    pub multi_select: bool,
}

/// One selectable option of a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One answered question: the selected option labels (and an optional free
/// text answer for questions that offered no options).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAnswer {
    pub id: String,
    pub selected: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

/// Bridge → client messages.
#[derive(Serialize, Deserialize, Debug)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ServerMessage {
    Welcome {
        #[serde(default)]
        protocol_version: Option<u64>,
        #[serde(default)]
        max_frame_bytes: Option<usize>,
        session_id: String,
        status: String,
        provider: Option<String>,
        model: Option<String>,
        /// Actual agent preset mounted for the attached session. Optional for
        /// compatibility with bridges that predate authoritative mode display.
        #[serde(default)]
        mode: Option<String>,
        /// Latest session/title of the attached session (absent until it
        /// has one; live updates ride ordinary `event` frames).
        #[serde(default)]
        title: Option<String>,
        /// Workspace path of the attached session (its header cwd), rendered
        /// right-aligned in the title row below the status bar.
        #[serde(default)]
        cwd: Option<String>,
    },
    Snapshot {
        events: Vec<HostEvent>,
        /// True when the bridge capped the replay window.
        #[serde(default)]
        truncated: bool,
    },
    Event {
        event: HostEvent,
    },
    Status {
        status: String,
    },
    /// One page of older history for the scroll-back request.
    History {
        events: Vec<HostEvent>,
        /// False when the returned page reaches the oldest stored event.
        #[serde(default)]
        has_more: bool,
    },
    Sessions {
        sessions: Vec<SessionInfo>,
        /// True for the fast header-only frame; a second frame follows after
        /// persisted title snapshots have been folded.
        #[serde(default)]
        titles_pending: bool,
    },
    /// The agent-preset roster the host offers; feeds the `/new <mode>`
    /// suggestion popup. Sent after every `welcome` (attach/`/new`/resume).
    Presets {
        presets: Vec<PresetInfo>,
    },
    /// User-invocable skills visible in the attached session's cwd/scope;
    /// feeds `/skill:<name>` argument completion.
    Skills {
        #[serde(default)]
        skills: Vec<SkillInfo>,
    },
    /// Title of the attached session, fetched from the projection store
    /// when the log is cold (resumed sessions) and the welcome frame could
    /// not carry one.
    Title {
        title: String,
    },
    /// Login page state: the model providers (API-key entries) and the saved
    /// proxy routes. Secret values never cross the wire — only
    /// configured/source/hint views.
    Login {
        /// Providers that authenticate with an API key, in roster order.
        #[serde(default)]
        providers: Vec<ProviderInfo>,
        /// Custom proxy provider routes the user has added.
        #[serde(default)]
        proxies: Vec<ProxyInfo>,
        /// Message of the last rejected write (absent after a success).
        #[serde(default)]
        error: Option<String>,
    },
    Approval {
        id: String,
        tool_name: String,
        reason: String,
        call_id: Option<String>,
    },
    /// One pending user-question batch to answer in the TUI.
    Question {
        rpc_id: String,
        session_id: String,
        questions: Vec<QuestionItem>,
    },
    /// A question batch settled (answered anywhere, or cancelled) — the TUI
    /// must drop its pending selection UI for that batch.
    QuestionResolved {
        question_rpc_id: String,
        outcome: String,
    },
    /// Effective DSH/plugin command directory for the attached agent. The
    /// client merges this with its optimized built-ins; built-ins shadow a
    /// same-name descriptor.
    Commands {
        #[serde(default)]
        commands: Vec<CommandInfo>,
    },
    /// Direct UI outcome of a generically executed DSH/plugin command.
    CommandResult {
        command_id: String,
        kind: String,
        #[serde(default)]
        text: Option<String>,
    },
    /// The provider/model catalog (for the `/model` picker) plus the current
    /// selection.
    Model {
        #[serde(default)]
        providers: Vec<ModelProviderInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<ModelCurrent>,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// One session in the `/resume` Input Page list.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub live: bool,
    pub created_at: u64,
}

/// One agent preset (a selectable `/new` mode) from the host roster.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u64>,
    /// Present when the preset's composition cannot mount — the client
    /// hides it from the mode popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken: Option<String>,
}

/// One user-invocable skill from the attached agent's effective registry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

/// One effective command discovered through DSH's `ctx.commands.list(agent)`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    /// Lowercase DSH command name without the leading slash.
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<CommandInputInfo>,
}

/// DSH currently exposes only an unstructured argument hint; it does not
/// expose a plugin argument-completion schema.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInputInfo {
    pub hint: String,
}

/// One API-key model provider on the login page.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub api_key_configured: bool,
    #[serde(default)]
    pub api_key_writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_hint: Option<String>,
}

/// One custom proxy provider route.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInfo {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub model: String,
}

/// One model provider in the `/model` picker, with its model catalog.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}

/// One selectable model in the `/model` picker.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The current provider/model selection.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelCurrent {
    pub provider: String,
    pub model: String,
}

impl ClientMessage {
    pub fn to_wire(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl ServerMessage {
    pub fn from_wire(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}
