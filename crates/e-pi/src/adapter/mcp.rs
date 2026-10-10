//! Correlated MCP controls in the existing Pi child; no second MCP client.

use std::time::{Duration, Instant};

use e_tui::{
    agent::AgentEvent,
    mcp::{McpEvent, McpOperation, McpPatch, McpRequest, McpSnapshot},
    AgentRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{AdapterOutput, PiAdapter};
use crate::protocol::{ExtensionUiResponse, RpcCommand, RpcRecord};

pub(super) const GET: &str = "__pie_mcp_get_v1";
pub(super) const SAVE: &str = "__pie_mcp_save_v1";
pub(super) const OPEN: &str = "__pie_mcp_open_v1";
const STATUS_KEY: &str = "pie-mcp-v1";

#[derive(Default)]
pub(super) struct McpAdapter {
    pub available: bool,
    pub pending: Option<Pending>,
    reload_required: bool,
    browser: Option<(String, String, Instant, bool)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Get,
    Save,
    Status,
    Action(McpOperation),
    Reload,
    AfterReload,
}

pub(super) struct Pending {
    id: String,
    wire_id: String,
    session: String,
    stage: Stage,
    control: Option<Control>,
    cancelled: bool,
    timed_out: bool,
    prompts: Vec<String>,
    notices: String,
    authorization_url: Option<String>,
    deadline: Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Control {
    protocol: u64,
    session_id: String,
    id: String,
    kind: String,
    success: bool,
    #[serde(default)]
    saved: bool,
    #[serde(default)]
    busy: bool,
    snapshot: Option<McpSnapshot>,
    error: Option<String>,
}

fn event(event: McpEvent) -> AgentEvent {
    AgentEvent::Mcp(event)
}

fn failed(id: String, message: impl Into<String>) -> AdapterOutput {
    AdapterOutput::event(event(McpEvent::Finished {
        id,
        error: Some(message.into()),
    }))
}

fn busy(adapter: &PiAdapter) -> bool {
    adapter.is_streaming
        || adapter.pending_command_prompt.is_some()
        || adapter.retry.busy()
        || adapter.active_compaction_id.is_some()
        || adapter.pending_compaction.is_some()
}

fn command(adapter: &mut PiAdapter, id: String, stage: Stage, message: String) -> AdapterOutput {
    let wire_id = adapter.request_id("mcp");
    adapter.configuration_request = Some(wire_id.clone());
    adapter.mcp.pending = Some(Pending {
        id,
        wire_id: wire_id.clone(),
        session: adapter.session_id.clone(),
        stage,
        control: None,
        cancelled: false,
        timed_out: false,
        prompts: vec![],
        notices: String::new(),
        authorization_url: None,
        deadline: Instant::now()
            + Duration::from_secs(if stage == Stage::Action(McpOperation::Login) {
                300
            } else {
                90
            }),
    });
    AdapterOutput::command(RpcCommand::Prompt {
        id: Some(wire_id),
        message,
        streaming_behavior: None,
    })
}

pub(super) fn request(adapter: &mut PiAdapter, request: McpRequest) -> AdapterOutput {
    match request {
        McpRequest::OpenUrl { id, url } => {
            let valid = adapter.mcp.pending.as_ref().is_some_and(|pending| {
                pending.id == id
                    && pending.session == adapter.session_id
                    && !pending.cancelled
                    && !pending.timed_out
                    && pending.authorization_url.as_deref() == Some(url.as_str())
            });
            if !valid || adapter.mcp.browser.is_some() {
                return AdapterOutput::default();
            }
            let wire_id = adapter.request_id("mcp-browser");
            adapter.mcp.browser = Some((
                id.clone(),
                wire_id.clone(),
                Instant::now() + Duration::from_secs(15),
                false,
            ));
            let args = json!({"id": id, "sessionId": adapter.session_id, "url": url});
            return AdapterOutput::command(RpcCommand::Prompt {
                id: Some(wire_id),
                message: format!("/{OPEN} {args}"),
                streaming_behavior: None,
            });
        }
        McpRequest::Reply {
            id,
            prompt_id,
            value,
        } => {
            let Some(pending) = adapter.mcp.pending.as_mut().filter(|pending| {
                pending.id == id && !pending.cancelled && pending.prompts.contains(&prompt_id)
            }) else {
                return AdapterOutput::default();
            };
            pending.prompts.retain(|id| *id != prompt_id);
            return AdapterOutput::command(RpcCommand::ExtensionUiResponse {
                id: prompt_id,
                response: ExtensionUiResponse::Value { value },
            });
        }
        McpRequest::Cancel { id } => {
            let Some(pending) = adapter
                .mcp
                .pending
                .as_mut()
                .filter(|pending| pending.id == id)
            else {
                return AdapterOutput::default();
            };
            pending.cancelled = true;
            return AdapterOutput {
                commands: pending
                    .prompts
                    .drain(..)
                    .map(|id| RpcCommand::ExtensionUiResponse {
                        id,
                        response: ExtensionUiResponse::Cancelled { cancelled: true },
                    })
                    .collect(),
                events: vec![],
            };
        }
        _ => {}
    }
    let id = match &request {
        McpRequest::Get { id } | McpRequest::Save { id, .. } | McpRequest::Action { id, .. } => {
            id.clone()
        }
        _ => unreachable!(),
    };
    if !adapter.mcp.available {
        return failed(
            id,
            "Native MCP management is unavailable or replaced by an extension.",
        );
    }
    if adapter.mcp.pending.is_some()
        || adapter.configuration_request.is_some()
        || adapter.pending_fork.is_some()
        || adapter.pending_queue.operation.is_some()
        || adapter.pending_skill_prompt.is_some()
    {
        return failed(
            id,
            "Wait for the current resource/session operation to finish.",
        );
    }
    match request {
        McpRequest::Get { id } => {
            let args = json!({"id": id, "sessionId": adapter.session_id});
            command(adapter, id, Stage::Get, format!("/{GET} {args}"))
        }
        McpRequest::Save {
            id,
            server,
            revision,
            patch,
        } => {
            let patch = match patch {
                McpPatch::Enabled(value) => json!({"enabled": value}),
                McpPatch::Exposure(value) => json!({"exposure": value}),
            };
            let args = json!({"id": id, "sessionId": adapter.session_id, "server": server,
                "revision": revision, "patch": patch, "deferReload": busy(adapter)});
            command(adapter, id, Stage::Save, format!("/{SAVE} {args}"))
        }
        McpRequest::Action {
            id,
            server,
            operation,
        } => {
            if busy(adapter) {
                return failed(
                    id,
                    "Wait for the current agent run to finish before MCP login/logout/reconnect.",
                );
            }
            if server.is_empty()
                || !server
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            {
                return failed(id, "Invalid MCP server identity.");
            }
            let action = match operation {
                McpOperation::Login => "login",
                McpOperation::Logout => "logout",
                McpOperation::Reconnect => "reconnect",
            };
            command(
                adapter,
                id,
                Stage::Action(operation),
                format!("/mcp {action} {server}"),
            )
        }
        _ => unreachable!(),
    }
}

pub(super) fn control(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    if record.string("method") != Some("setStatus")
        || record.string("statusKey") != Some(STATUS_KEY)
    {
        return None;
    }
    let text = record.string("statusText").unwrap_or_default();
    if let Some((id, _, _, success)) = &mut adapter.mcp.browser {
        if let Ok(value) = serde_json::from_str::<Control>(text) {
            if value.kind == "open" {
                if value.protocol == 1 && value.id == *id && value.session_id == adapter.session_id
                {
                    *success = value.success;
                }
                return Some(AdapterOutput::default());
            }
        }
    }
    let Some(pending) = adapter.mcp.pending.as_mut() else {
        return Some(AdapterOutput::default());
    };
    let value = (text.len() <= 256 * 1024)
        .then(|| serde_json::from_str::<Control>(text).ok())
        .flatten();
    let Some(control) = value.filter(|control| {
        control.protocol == 1
            && control.id == pending.id
            && control.session_id == pending.session
            && control.session_id == adapter.session_id
            && control.kind
                == if pending.stage == Stage::Save {
                    "save"
                } else {
                    "get"
                }
    }) else {
        return Some(AdapterOutput::default());
    };
    pending.control = Some(control);
    Some(AdapterOutput::default())
}

pub(super) fn ui(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    if let Some(output) = control(adapter, record) {
        return Some(output);
    }
    let pending = adapter.mcp.pending.as_mut()?;
    if !matches!(pending.stage, Stage::Status | Stage::Action(_)) {
        return None;
    }
    match record.string("method") {
        Some("notify") => {
            let message = record.string("message").unwrap_or_default();
            // RPC notifications have no command correlation. Only take MCP-shaped guidance.
            if pending.stage != Stage::Action(McpOperation::Login)
                && !message.contains("MCP")
                && !message.contains("Sign-in")
                && !message.contains("sign-in")
                && !message.contains("tools")
                && !message.contains("(codemode)")
                && !message.contains("(deferred)")
                && !message.contains("(direct)")
                && !message.contains("(hidden)")
            {
                return None;
            }
            if pending.cancelled || pending.timed_out {
                return Some(AdapterOutput::default());
            }
            let message = if pending.stage == Stage::Action(McpOperation::Login)
                && record.string("notifyType") == Some("error")
            {
                "Native MCP sign-in failed or was cancelled.".to_owned()
            } else {
                message.chars().take(16 * 1024).collect()
            };
            if pending.notices.len() < 16 * 1024 {
                if !pending.notices.is_empty() {
                    pending.notices.push('\n');
                }
                pending.notices.push_str(&message);
            }
            let mut output = AdapterOutput::default();
            if pending.stage == Stage::Action(McpOperation::Login) {
                let url = e_tui::link_copy::discover(&message)
                    .into_iter()
                    .flat_map(|group| group.alternatives)
                    .find(|candidate| {
                        matches!(candidate.kind, e_tui::link_copy::LinkTargetKind::Uri)
                            && (candidate.target.starts_with("https://")
                                || candidate.target.starts_with("http://"))
                    })
                    .map(|candidate| candidate.target);
                if let Some(url) = url {
                    pending.authorization_url = Some(url.clone());
                    output.events.push(event(McpEvent::Authorization {
                        id: pending.id.clone(),
                        url,
                    }));
                }
            }
            output.events.push(event(if pending.stage == Stage::Status {
                McpEvent::Status {
                    id: pending.id.clone(),
                    message: pending.notices.clone(),
                }
            } else {
                McpEvent::Notice {
                    id: pending.id.clone(),
                    message: pending.notices.clone(),
                }
            }));
            Some(output)
        }
        Some("input") if pending.stage == Stage::Action(McpOperation::Login) => {
            let prompt_id = record.string("id")?.to_owned();
            if pending.cancelled || pending.timed_out {
                return Some(AdapterOutput::command(RpcCommand::ExtensionUiResponse {
                    id: prompt_id,
                    response: ExtensionUiResponse::Cancelled { cancelled: true },
                }));
            }
            pending.prompts.push(prompt_id.clone());
            Some(AdapterOutput::event(event(McpEvent::Prompt {
                id: pending.id.clone(),
                prompt_id,
                message: record
                    .string("title")
                    .unwrap_or("Paste callback URL (masked)")
                    .into(),
            })))
        }
        _ => None,
    }
}

fn saved_notice(adapter: &mut PiAdapter, message: &str) -> AdapterOutput {
    adapter.mcp.reload_required = true;
    let mut output = AdapterOutput::event(event(McpEvent::Saved {
        message: message.into(),
    }));
    output.events.push(event(McpEvent::ReloadRequired(true)));
    output
}

pub(super) fn response(adapter: &mut PiAdapter, record: &RpcRecord) -> Option<AdapterOutput> {
    if record
        .string("id")
        .is_some_and(|id| id.starts_with("pie-mcp-browser-"))
    {
        let matches = adapter
            .mcp
            .browser
            .as_ref()
            .is_some_and(|(_, wire_id, _, _)| record.string("id") == Some(wire_id.as_str()));
        if !matches {
            return Some(AdapterOutput::default());
        }
        let (id, _, _, succeeded) = adapter.mcp.browser.take().expect("browser request");
        return Some(AdapterOutput::event(event(McpEvent::Notice {
            id,
            message: if succeeded && record.bool("success") == Some(true) {
                "Browser launch requested."
            } else {
                "Unable to open the browser. Copy the authorization link instead."
            }
            .into(),
        })));
    }
    let matches = adapter.mcp.pending.as_ref().is_some_and(|pending| {
        pending.stage != Stage::Reload && record.string("id") == Some(pending.wire_id.as_str())
    });
    if !matches {
        return None;
    }
    let mut pending = adapter.mcp.pending.take().expect("MCP response owner");
    if adapter.configuration_request.as_deref() == Some(&pending.wire_id) {
        adapter.configuration_request = None;
    }
    if pending.session != adapter.session_id {
        return Some(AdapterOutput::default());
    }
    if pending.timed_out {
        let mut output = AdapterOutput::default();
        if pending
            .control
            .as_ref()
            .is_some_and(|control| control.saved)
        {
            output.merge(saved_notice(adapter, "MCP configuration was saved after the operation timed out. Run /reload when idle to apply it."));
        }
        super::response::drain_deferred(adapter, &mut output);
        return Some(output);
    }
    let mut output = AdapterOutput::default();
    let success = record.bool("success") == Some(true)
        && record
            .field("data")
            .and_then(|data| data.get("disposition"))
            .and_then(Value::as_str)
            == Some("handled");
    if !success {
        output.merge(failed(
            pending.id,
            "Native MCP command was not handled successfully.",
        ));
    } else if matches!(pending.stage, Stage::Get | Stage::Save | Stage::AfterReload) {
        let Some(control) = pending.control.take() else {
            output.merge(failed(
                pending.id,
                "MCP companion did not return a supported response.",
            ));
            super::response::drain_deferred(adapter, &mut output);
            return Some(output);
        };
        if control.saved {
            output.events.push(event(McpEvent::ReloadRequired(true)));
            adapter.mcp.reload_required = true;
        }
        if let Some(snapshot) = control.snapshot {
            output.events.push(event(McpEvent::Snapshot {
                id: pending.id.clone(),
                snapshot,
            }));
        }
        if !control.success {
            if control.saved {
                output.merge(saved_notice(
                    adapter,
                    "MCP configuration saved. Run /reload to apply it; display refresh failed.",
                ));
            }
            output.merge(failed(
                pending.id,
                control
                    .error
                    .unwrap_or_else(|| "MCP configuration operation failed.".into()),
            ));
        } else if pending.stage == Stage::Save {
            if control.busy || busy(adapter) {
                let message = "MCP configuration saved. The agent is running; run /reload when idle to apply it.";
                output.merge(saved_notice(adapter, message));
                output.events.push(event(McpEvent::Notice {
                    id: pending.id.clone(),
                    message: message.into(),
                }));
                output.events.push(event(McpEvent::Finished {
                    id: pending.id,
                    error: None,
                }));
            } else {
                pending.stage = Stage::Reload;
                pending.deadline = Instant::now() + Duration::from_secs(90);
                adapter.mcp.pending = Some(pending);
                output.merge(super::request::route(
                    adapter,
                    AgentRequest::Command {
                        line: "/reload".into(),
                        images: vec![],
                    },
                ));
            }
        } else {
            let wire_id = adapter.request_id("mcp-status");
            pending.wire_id = wire_id.clone();
            pending.stage = Stage::Status;
            adapter.configuration_request = Some(wire_id.clone());
            adapter.mcp.pending = Some(pending);
            output.commands.push(RpcCommand::Prompt {
                id: Some(wire_id),
                message: "/mcp".into(),
                streaming_behavior: None,
            });
        }
    } else if pending.stage == Stage::Status || pending.cancelled {
        output.events.push(event(McpEvent::Finished {
            id: pending.id,
            error: None,
        }));
    } else {
        // Native mutation outcome is conveyed by its notifications, not prompt acceptance.
        let id = pending.id;
        let args = json!({"id": id, "sessionId": adapter.session_id});
        output.merge(command(adapter, id, Stage::Get, format!("/{GET} {args}")));
    }
    super::response::drain_deferred(adapter, &mut output);
    Some(output)
}

pub(super) fn reload_finished(adapter: &mut PiAdapter, success: bool) -> AdapterOutput {
    let mut output = AdapterOutput::default();
    if success {
        adapter.mcp.reload_required = false;
        output.events.push(event(McpEvent::ReloadRequired(false)));
    }
    if !adapter
        .mcp
        .pending
        .as_ref()
        .is_some_and(|pending| pending.stage == Stage::Reload)
    {
        return output;
    }
    let pending = adapter.mcp.pending.take().expect("MCP reload owner");
    if success {
        let args = json!({"id": pending.id, "sessionId": adapter.session_id});
        output.merge(command(
            adapter,
            pending.id,
            Stage::AfterReload,
            format!("/{GET} {args}"),
        ));
        output.events.push(event(McpEvent::Saved {
            message: "MCP configuration saved and Pi resources reloaded.".into(),
        }));
    } else {
        let message = "MCP configuration saved, but reload did not complete. Run /reload when the agent is idle.";
        output.merge(saved_notice(adapter, message));
        output.merge(failed(pending.id, message));
    }
    output
}

pub(super) fn deadline(adapter: &PiAdapter) -> Option<Instant> {
    let operation = adapter
        .mcp
        .pending
        .as_ref()
        .filter(|pending| !pending.timed_out)
        .map(|pending| pending.deadline);
    operation
        .into_iter()
        .chain(
            adapter
                .mcp
                .browser
                .as_ref()
                .map(|(_, _, deadline, _)| *deadline),
        )
        .min()
}

pub(super) fn tick(adapter: &mut PiAdapter, now: Instant) -> AdapterOutput {
    let mut output = AdapterOutput::default();
    if adapter
        .mcp
        .browser
        .as_ref()
        .is_some_and(|(_, _, deadline, _)| now >= *deadline)
    {
        let (id, _, _, _) = adapter.mcp.browser.take().expect("expired browser request");
        output.events.push(event(McpEvent::Notice {
            id,
            message: "Browser launch did not settle. Copy the authorization link instead.".into(),
        }));
    }
    let Some(pending) = adapter
        .mcp
        .pending
        .as_mut()
        .filter(|pending| !pending.timed_out && now >= pending.deadline)
    else {
        return output;
    };
    pending.timed_out = true;
    output.merge(failed(pending.id.clone(), "Native MCP operation timed out; its outcome may be uncertain. Do not retry a credential change without checking native status."));
    output.commands.extend(
        pending
            .prompts
            .drain(..)
            .map(|id| RpcCommand::ExtensionUiResponse {
                id,
                response: ExtensionUiResponse::Cancelled { cancelled: true },
            }),
    );
    if matches!(
        pending.stage,
        Stage::Get | Stage::Status | Stage::AfterReload
    ) && adapter.configuration_request.as_deref() == Some(&pending.wire_id)
    {
        adapter.configuration_request = None;
    }
    super::response::drain_deferred(adapter, &mut output);
    output
}
