//! Provider-neutral MCP messages and inspection metadata.

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSnapshot {
    pub revision: String,
    pub trusted: bool,
    pub errors: Vec<String>,
    pub servers: Vec<McpServer>,
    pub exposures: Vec<McpExposureMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct McpExposureMode {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    pub enabled: bool,
    pub exposure: String,
    pub source: String,
    pub writable: bool,
    pub transport: String,
    pub description: String,
    pub overrides: usize,
    pub tool_count: usize,
    pub tools: Vec<McpTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub exposure: String,
    pub schema: String,
    pub annotations: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpPatch {
    Enabled(bool),
    Exposure(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpOperation {
    Login,
    Logout,
    Reconnect,
}

#[derive(Clone, PartialEq, Eq)]
pub enum McpRequest {
    Get {
        id: String,
    },
    Save {
        id: String,
        server: String,
        revision: String,
        patch: McpPatch,
    },
    Action {
        id: String,
        server: String,
        operation: McpOperation,
    },
    OpenUrl {
        id: String,
        url: String,
    },
    Reply {
        id: String,
        prompt_id: String,
        value: String,
    },
    Cancel {
        id: String,
    },
}

impl std::fmt::Debug for McpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get { id } => f.debug_tuple("Get").field(id).finish(),
            Self::Save {
                id, server, patch, ..
            } => f
                .debug_tuple("Save")
                .field(id)
                .field(server)
                .field(patch)
                .finish(),
            Self::Action {
                id,
                server,
                operation,
            } => f
                .debug_tuple("Action")
                .field(id)
                .field(server)
                .field(operation)
                .finish(),
            Self::OpenUrl { id, .. } => f
                .debug_tuple("OpenUrl")
                .field(id)
                .field(&"<authorization link>")
                .finish(),
            Self::Reply { id, prompt_id, .. } => f
                .debug_tuple("Reply")
                .field(id)
                .field(prompt_id)
                .field(&"<redacted>")
                .finish(),
            Self::Cancel { id } => f.debug_tuple("Cancel").field(id).finish(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum McpEvent {
    Available(bool),
    Saved {
        message: String,
    },
    Snapshot {
        id: String,
        snapshot: McpSnapshot,
    },
    Notice {
        id: String,
        message: String,
    },
    Status {
        id: String,
        message: String,
    },
    Authorization {
        id: String,
        url: String,
    },
    Prompt {
        id: String,
        prompt_id: String,
        message: String,
    },
    Finished {
        id: String,
        error: Option<String>,
    },
    ReloadRequired(bool),
}
