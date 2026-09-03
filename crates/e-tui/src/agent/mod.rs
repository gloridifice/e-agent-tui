//! Facts observed from an agent kernel or completed executable effect.

pub mod timeline;
pub mod tool;

pub use timeline::*;
pub use tool::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Waiting,
    Error,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedSession {
    pub protocol_version: Option<u64>,
    pub max_frame_bytes: Option<usize>,
    pub id: String,
    pub status: AgentStatus,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub title: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub live: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Attached(AttachedSession),
    Status(AgentStatus),
    Title(String),
    List {
        sessions: Vec<SessionSummary>,
        titles_pending: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TimelineEvent {
    Snapshot {
        records: Vec<TimelineRecord>,
        truncated: bool,
    },
    Append(TimelineRecord),
    History {
        records: Vec<TimelineRecord>,
        has_more: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub order: Option<u64>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
}

pub use crate::command_catalog::CommandDescriptor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialProvider {
    pub id: String,
    pub name: String,
    pub api_key_configured: bool,
    pub api_key_writable: bool,
    pub api_key_source: Option<String>,
    pub api_key_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyRoute {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub reasoning: Option<ModelReasoning>,
}

/// Selectable reasoning metadata for one exact provider/model route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReasoning {
    pub efforts: Vec<ReasoningEffort>,
    pub default_effort: Option<String>,
}

/// One adapter-owned reasoning effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningEffort {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogEvent {
    Presets(Vec<Preset>),
    Skills(Vec<Skill>),
    Commands(Vec<CommandDescriptor>),
    Login {
        providers: Vec<CredentialProvider>,
        proxies: Vec<ProxyRoute>,
        error: Option<String>,
    },
    Models {
        providers: Vec<ModelProvider>,
        current: Option<ModelSelection>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub question: String,
    pub header: Option<String>,
    pub options: Option<Vec<QuestionOption>>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionEvent {
    CommandResult {
        id: String,
        outcome: String,
        text: Option<String>,
    },
    Approval {
        id: String,
        capability: ToolCapability,
        label: String,
        reason: String,
    },
    Question {
        request_id: String,
        session_id: String,
        questions: Vec<Question>,
    },
    QuestionResolved {
        request_id: String,
        outcome: String,
    },
    Error {
        code: String,
        message: String,
    },
    /// Replace the visible ordinary composer text at an adapter's request.
    /// Hidden Input Page editors retain their own drafts.
    SetEditorText {
        text: String,
    },
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewEvent {
    Resolved {
        request_id: crate::preview::PreviewRequestId,
        key: crate::preview::PreviewKey,
        revision: crate::preview::PreviewRevision,
        result: Result<crate::preview::PreviewContent, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineEvent {
    Animation,
    Frame,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Session(SessionEvent),
    Timeline(TimelineEvent),
    Catalog(CatalogEvent),
    Interaction(InteractionEvent),
    Preview(PreviewEvent),
    EffectCompleted(crate::action::EffectResult),
    Deadline(DeadlineEvent),
}
