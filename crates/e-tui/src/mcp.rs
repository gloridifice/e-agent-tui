//! Provider-neutral MCP configuration modal and safe inspection metadata.

use serde::Deserialize;

use crate::{
    key_mapping::{Action, MappedKey, Scope},
    page_core::{handle_text_input, TextEditResult, TextEditor},
    AgentRequest, UiAction,
};

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

#[derive(Debug, Clone)]
pub enum McpView {
    Servers,
    Tools(String),
    Detail(String),
    Exposure(String),
    Confirm {
        server: String,
        patch: Option<McpPatch>,
    },
    Login,
}

pub struct McpState {
    pub snapshot: McpSnapshot,
    pub view: McpView,
    pub pos: usize,
    pub filter: String,
    pub editor: Option<TextEditor>,
    pub callback: Option<String>,
    pub pending: Option<String>,
    pub notice: String,
    pub native_status: String,
    pub authorization_url: Option<String>,
    last_native_notice: Option<String>,
    pub detail_scroll: usize,
    pub detail_tool: Option<usize>,
    pub viewport_rows: usize,
    prefix: u64,
    sequence: u64,
}

impl McpState {
    pub fn open(prefix: u64) -> (Self, AgentRequest) {
        let mut state = Self {
            snapshot: McpSnapshot::default(),
            view: McpView::Servers,
            pos: 0,
            filter: String::new(),
            editor: None,
            callback: None,
            pending: None,
            notice: "Reading MCP configuration…".into(),
            native_status: String::new(),
            authorization_url: None,
            last_native_notice: None,
            detail_scroll: 0,
            detail_tool: None,
            viewport_rows: 1,
            prefix,
            sequence: 0,
        };
        let id = state.next_id();
        state.pending = Some(id.clone());
        (state, AgentRequest::Mcp(McpRequest::Get { id }))
    }

    pub fn open_action(
        prefix: u64,
        server: String,
        operation: McpOperation,
    ) -> (Self, AgentRequest) {
        let (mut state, _) = Self::open(prefix);
        state.notice = "Waiting for native MCP…".into();
        if operation == McpOperation::Login {
            state.view = McpView::Login;
        }
        let request = AgentRequest::Mcp(McpRequest::Action {
            id: state.pending.clone().expect("new modal request"),
            server,
            operation,
        });
        (state, request)
    }

    fn next_id(&mut self) -> String {
        self.sequence += 1;
        format!("mcp-{}-{}", self.prefix, self.sequence)
    }

    pub fn key_scope(&self) -> Scope {
        if self.editor.is_some() {
            Scope::PageEdit
        } else {
            Scope::Mcp
        }
    }

    pub fn visible_servers(&self) -> Vec<&McpServer> {
        let query = self.filter.to_lowercase();
        self.snapshot
            .servers
            .iter()
            .filter(|server| {
                format!("{} {}", server.name, server.description)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect()
    }

    pub fn server(&self) -> Option<&McpServer> {
        let name = match &self.view {
            McpView::Tools(name)
            | McpView::Detail(name)
            | McpView::Exposure(name)
            | McpView::Confirm { server: name, .. } => Some(name.as_str()),
            _ => None,
        };
        name.map_or_else(
            || self.visible_servers().get(self.pos).copied(),
            |name| {
                self.snapshot
                    .servers
                    .iter()
                    .find(|server| server.name == name)
            },
        )
    }

    pub fn apply(&mut self, event: McpEvent) {
        match event {
            McpEvent::Snapshot { id, snapshot } if self.pending.as_ref() == Some(&id) => {
                let selected = self.server().map(|server| server.name.clone());
                self.snapshot = snapshot;
                if let Some(name) = selected {
                    if matches!(self.view, McpView::Servers) {
                        self.pos = self
                            .visible_servers()
                            .iter()
                            .position(|server| server.name == name)
                            .unwrap_or(0);
                    }
                }
                self.notice = if self.snapshot.errors.is_empty() {
                    self.last_native_notice.clone().unwrap_or_default()
                } else {
                    self.snapshot.errors.join("\n")
                };
            }
            McpEvent::Notice { id, message } if self.pending.as_ref() == Some(&id) => {
                self.last_native_notice = Some(message.clone());
                self.notice = message;
            }
            McpEvent::Authorization { id, url } if self.pending.as_ref() == Some(&id) => {
                self.authorization_url = Some(url);
                self.view = McpView::Login;
            }
            McpEvent::Status { id, message } if self.pending.as_ref() == Some(&id) => {
                self.native_status = message;
            }
            McpEvent::Prompt {
                id,
                prompt_id,
                message,
            } if self.pending.as_ref() == Some(&id) => {
                self.view = McpView::Login;
                self.callback = Some(prompt_id);
                self.editor = Some(TextEditor {
                    buf: String::new(),
                    secret: true,
                });
                self.notice = message;
            }
            McpEvent::Finished { id, error } if self.pending.as_ref() == Some(&id) => {
                self.pending = None;
                self.callback = None;
                self.editor = None;
                self.authorization_url = None;
                if let Some(error) = error {
                    self.notice = error;
                }
                if matches!(self.view, McpView::Login) {
                    self.view = McpView::Servers;
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(editor) = &mut self.editor {
            if editor.buf.len().saturating_add(text.len()) <= 16 * 1024 {
                editor.buf.push_str(text);
                return true;
            }
        }
        false
    }

    pub fn cancel_request(&self) -> Option<AgentRequest> {
        self.pending
            .as_ref()
            .map(|id| AgentRequest::Mcp(McpRequest::Cancel { id: id.clone() }))
    }

    pub fn handle_input(&mut self, key: MappedKey) -> (bool, Vec<UiAction>) {
        let send = |request| vec![UiAction::Agent(AgentRequest::Mcp(request))];
        if matches!(self.view, McpView::Login) {
            match key {
                MappedKey::Command(Action::CopyLink) => {
                    return (
                        false,
                        self.authorization_url
                            .clone()
                            .map(UiAction::WriteClipboard)
                            .into_iter()
                            .collect(),
                    );
                }
                MappedKey::Command(Action::OpenLink) => {
                    if let (Some(id), Some(url)) =
                        (self.pending.clone(), self.authorization_url.clone())
                    {
                        return (false, send(McpRequest::OpenUrl { id, url }));
                    }
                    return (false, vec![]);
                }
                _ => {}
            }
        }
        if let Some(mut editor) = self.editor.take() {
            return match handle_text_input(&mut editor, key) {
                TextEditResult::Confirm(value) => {
                    if let Some(prompt_id) = self.callback.take() {
                        self.notice = "Waiting for native sign-in…".into();
                        (
                            false,
                            send(McpRequest::Reply {
                                id: self.pending.clone().unwrap_or_default(),
                                prompt_id,
                                value,
                            }),
                        )
                    } else {
                        self.filter = value;
                        self.pos = 0;
                        (false, vec![])
                    }
                }
                TextEditResult::Cancel => {
                    self.callback = None;
                    (
                        false,
                        self.cancel_request()
                            .map(UiAction::Agent)
                            .into_iter()
                            .collect(),
                    )
                }
                TextEditResult::Continue => {
                    editor
                        .buf
                        .truncate(editor.buf.floor_char_boundary(16 * 1024));
                    self.editor = Some(editor);
                    (false, vec![])
                }
            };
        }
        let MappedKey::Command(action) = key else {
            return (false, vec![]);
        };
        if action == Action::Close
            || (action == Action::Back && matches!(self.view, McpView::Servers))
        {
            return (
                true,
                self.cancel_request()
                    .map(UiAction::Agent)
                    .into_iter()
                    .collect(),
            );
        }
        if action == Action::Back {
            let cancel = matches!(self.view, McpView::Login)
                .then(|| self.cancel_request())
                .flatten();
            self.view = McpView::Servers;
            self.pos = 0;
            self.detail_scroll = 0;
            return (false, cancel.map(UiAction::Agent).into_iter().collect());
        }
        let rows = match &self.view {
            McpView::Servers => self.visible_servers().len(),
            McpView::Tools(_) => self.server().map_or(0, |server| server.tools.len()),
            McpView::Exposure(_) => self.snapshot.exposures.len(),
            McpView::Confirm { .. } => 2,
            _ => 0,
        };
        match action {
            Action::MoveUp if matches!(self.view, McpView::Detail(_)) => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1)
            }
            Action::MoveDown if matches!(self.view, McpView::Detail(_)) => {
                self.detail_scroll = self.detail_scroll.saturating_add(1)
            }
            Action::MoveUp => {
                self.pos = self.pos.saturating_sub(1);
                self.detail_scroll = 0;
            }
            Action::MoveDown => {
                self.pos = (self.pos + 1).min(rows.saturating_sub(1));
                self.detail_scroll = 0;
            }
            Action::MoveUpFast => {
                self.detail_scroll = self.detail_scroll.saturating_sub(self.viewport_rows)
            }
            Action::MoveDownFast => {
                self.detail_scroll = self.detail_scroll.saturating_add(self.viewport_rows)
            }
            Action::Search if matches!(self.view, McpView::Servers) => {
                self.editor = Some(TextEditor {
                    buf: self.filter.clone(),
                    secret: false,
                });
            }
            Action::Refresh if self.pending.is_none() => {
                let id = self.next_id();
                self.pending = Some(id.clone());
                self.notice = "Refreshing MCP…".into();
                self.last_native_notice = None;
                return (false, send(McpRequest::Get { id }));
            }
            Action::Confirm if self.pending.is_none() => match self.view.clone() {
                McpView::Servers => {
                    if let Some(server) = self.server() {
                        self.view = McpView::Tools(server.name.clone());
                        self.pos = 0;
                    }
                }
                McpView::Exposure(server) => {
                    let Some(mode) = self.snapshot.exposures.get(self.pos) else {
                        return (false, vec![]);
                    };
                    self.view = McpView::Confirm {
                        server,
                        patch: Some(McpPatch::Exposure(mode.id.clone())),
                    };
                    self.pos = 0;
                }
                McpView::Confirm { server, patch } if self.pos == 1 => {
                    let id = self.next_id();
                    self.pending = Some(id.clone());
                    self.view = McpView::Servers;
                    self.pos = 0;
                    self.notice = if patch.is_some() {
                        "Saving MCP change…"
                    } else {
                        "Waiting for native sign-out…"
                    }
                    .into();
                    return (
                        false,
                        send(match patch {
                            Some(patch) => McpRequest::Save {
                                id,
                                server,
                                revision: self.snapshot.revision.clone(),
                                patch,
                            },
                            None => McpRequest::Action {
                                id,
                                server,
                                operation: McpOperation::Logout,
                            },
                        }),
                    );
                }
                McpView::Confirm { .. } => {
                    self.view = McpView::Servers;
                    self.pos = 0;
                }
                _ => {}
            },
            Action::Inspect
            | Action::EditExposure
            | Action::ToggleEnabled
            | Action::Login
            | Action::Logout
            | Action::Reconnect
                if self.pending.is_none() =>
            {
                let Some(server) = self.server().cloned() else {
                    return (false, vec![]);
                };
                if matches!(action, Action::EditExposure | Action::ToggleEnabled)
                    && !server.writable
                {
                    self.notice = "This server is read-only. Edit its defining extension or fix the configuration.".into();
                    return (false, vec![]);
                }
                match action {
                    Action::Inspect => {
                        self.detail_tool =
                            matches!(self.view, McpView::Tools(_)).then_some(self.pos);
                        self.view = McpView::Detail(server.name);
                        self.detail_scroll = 0;
                    }
                    Action::EditExposure => {
                        if self.snapshot.exposures.is_empty() {
                            return (false, vec![]);
                        }
                        self.pos = self
                            .snapshot
                            .exposures
                            .iter()
                            .position(|mode| mode.id == server.exposure)
                            .unwrap_or(0);
                        self.view = McpView::Exposure(server.name);
                    }
                    Action::ToggleEnabled => {
                        self.view = McpView::Confirm {
                            server: server.name,
                            patch: Some(McpPatch::Enabled(!server.enabled)),
                        };
                        self.pos = 0;
                    }
                    Action::Logout => {
                        self.view = McpView::Confirm {
                            server: server.name,
                            patch: None,
                        };
                        self.pos = 0;
                    }
                    Action::Login | Action::Reconnect => {
                        let id = self.next_id();
                        self.pending = Some(id.clone());
                        self.notice = "Waiting for native MCP…".into();
                        if action == Action::Login {
                            self.view = McpView::Login;
                        }
                        return (
                            false,
                            send(McpRequest::Action {
                                id,
                                server: server.name,
                                operation: if action == Action::Login {
                                    McpOperation::Login
                                } else {
                                    McpOperation::Reconnect
                                },
                            }),
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        (false, vec![])
    }
}
