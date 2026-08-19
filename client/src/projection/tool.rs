//! Tool and folded-file projection into public activity rows.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{
    display::{ActivityContinuation, ActivityRow, ActivityState, DisplayId},
    protocol::{HostEvent, HostEventKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileAction {
    Read,
    View,
    Edit,
    Replace,
    Insert,
    Create,
}

impl FileAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::View => "view",
            Self::Edit => "edit",
            Self::Replace => "replace",
            Self::Insert => "insert",
            Self::Create => "create",
        }
    }

    fn foldable(self) -> bool {
        self != Self::Create
    }
}

#[derive(Debug, Clone)]
pub struct FileItemProjection {
    pub action: FileAction,
    pub call_id: String,
    pub file: String,
    pub ok: Option<bool>,
}

#[derive(Debug, Clone)]
struct FileItem {
    action: FileAction,
    call_id: String,
    file: String,
    ok: Option<bool>,
}

#[derive(Debug, Clone)]
struct ToolCallState {
    row_id: DisplayId,
    start_ms: u64,
    create: bool,
}

#[derive(Debug, Clone)]
struct FileGroupState {
    id: DisplayId,
    items: Vec<FileItem>,
    start_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolMutation {
    Upsert(ActivityRow),
    MissingResult,
    /// Interaction-only tools have their own Input Page and never enter the
    /// transcript activity surface.
    Ignore,
}

#[derive(Debug, Default)]
pub struct ToolProjectionState {
    calls: HashMap<String, ToolCallState>,
    ignored_calls: HashSet<String>,
    groups: HashMap<DisplayId, FileGroupState>,
    open_group: Option<DisplayId>,
    next_group: u64,
}

impl ToolProjectionState {
    pub fn close_group(&mut self) {
        self.open_group = None;
    }

    pub fn project_call(
        &mut self,
        event: &HostEvent,
        session_cwd: Option<&str>,
        merge_files: bool,
        now_ms: u64,
    ) -> Option<ToolMutation> {
        let HostEventKind::ToolCall {
            call_id,
            name,
            arguments,
        } = &event.kind
        else {
            return None;
        };
        if name == "ask_user_question" {
            self.close_group();
            self.ignored_calls.insert(call_id.clone());
            return Some(ToolMutation::Ignore);
        }
        let file = classify_file_call(name, arguments, session_cwd);
        if let Some((action, path)) = file
            .as_ref()
            .filter(|(action, _)| action.foldable() && merge_files)
        {
            let group_id = if let Some(id) = self.open_group.clone() {
                id
            } else {
                let id = DisplayId::correlated("file-group", &self.next_group.to_string());
                self.next_group = self.next_group.wrapping_add(1);
                self.groups.insert(
                    id.clone(),
                    FileGroupState {
                        id: id.clone(),
                        items: Vec::new(),
                        start_ms: now_ms,
                    },
                );
                self.open_group = Some(id.clone());
                id
            };
            let group = self.groups.get_mut(&group_id).expect("open group exists");
            group.items.push(FileItem {
                action: *action,
                call_id: call_id.clone(),
                file: path.clone(),
                ok: None,
            });
            self.calls.insert(
                call_id.clone(),
                ToolCallState {
                    row_id: group_id,
                    start_ms: now_ms,
                    create: false,
                },
            );
            return Some(ToolMutation::Upsert(file_group_row(group, now_ms)));
        }

        self.close_group();
        let (label, summary, create) = file
            .map(|(action, path)| {
                (
                    action.label().to_owned(),
                    path,
                    action == FileAction::Create,
                )
            })
            .unwrap_or_else(|| (name.clone(), tool_summary(name, arguments), false));
        let id = DisplayId::correlated("tool-call", call_id);
        self.calls.insert(
            call_id.clone(),
            ToolCallState {
                row_id: id.clone(),
                start_ms: now_ms,
                create,
            },
        );
        let mut row = ActivityRow::root(id, label);
        row.summary = summary;
        row.start_ms = Some(now_ms);
        if !create {
            row.output_lines = Some(0);
            row.live_duration_since = Some(std::time::Instant::now());
        }
        Some(ToolMutation::Upsert(row))
    }

    pub fn project_result(&mut self, event: &HostEvent, now_ms: u64) -> Option<ToolMutation> {
        let HostEventKind::ToolResult {
            call_id,
            output,
            is_error,
            output_truncated,
        } = &event.kind
        else {
            return None;
        };
        if self.ignored_calls.remove(call_id) {
            return Some(ToolMutation::Ignore);
        }
        let Some(call) = self.calls.get(call_id).cloned() else {
            return Some(ToolMutation::MissingResult);
        };
        let ok = exit_marker(output) == 0 && !is_error;
        if let Some(group) = self.groups.get_mut(&call.row_id) {
            if let Some(item) = group.items.iter_mut().find(|item| item.call_id == *call_id) {
                item.ok = Some(ok);
            }
            return Some(ToolMutation::Upsert(file_group_row(group, now_ms)));
        }

        let mut row = ActivityRow::root(call.row_id, "tool");
        // The existing row supplies label/summary during merge in AppState.
        row.state = if ok {
            ActivityState::Success
        } else {
            ActivityState::Failure
        };
        row.start_ms = Some(call.start_ms);
        if !call.create {
            row.duration_ms = Some(now_ms.saturating_sub(call.start_ms));
            row.output_lines = Some(output.lines().count());
            row.output_lines_truncated = *output_truncated;
        }
        Some(ToolMutation::Upsert(row))
    }

    pub fn row_for_call(&self, call_id: &str) -> Option<&DisplayId> {
        self.calls.get(call_id).map(|call| &call.row_id)
    }

    pub fn group_items(&self, id: &DisplayId) -> Option<Vec<FileItemProjection>> {
        self.groups.get(id).map(|group| {
            group
                .items
                .iter()
                .map(|item| FileItemProjection {
                    action: item.action,
                    call_id: item.call_id.clone(),
                    file: item.file.clone(),
                    ok: item.ok,
                })
                .collect()
        })
    }
}

fn file_group_row(group: &FileGroupState, now_ms: u64) -> ActivityRow {
    let mut row = ActivityRow::root(group.id.clone(), "files");
    row.start_ms = Some(group.start_ms);
    let mut items = group.items.iter();
    if let Some(first) = items.next() {
        row.label = first.action.label().into();
        row.summary = first.file.clone();
    }
    row.continuations = items
        .map(|item| ActivityContinuation {
            separator: "; ".into(),
            label: item.action.label().into(),
            summary: item.file.clone(),
        })
        .collect();
    row.state = if group.items.iter().any(|item| item.ok.is_none()) {
        ActivityState::Running
    } else if group.items.iter().all(|item| item.ok == Some(true)) {
        ActivityState::Success
    } else {
        ActivityState::Failure
    };
    if !row.state.is_active() {
        row.duration_ms = Some(now_ms.saturating_sub(group.start_ms));
    }
    row
}

fn exit_marker(output: &str) -> i64 {
    for line in output.lines().rev().take(4) {
        if let Some(pos) = line.find("[exit code: ") {
            let tail = &line[pos + 12..];
            if let Some(end) = tail.find(']') {
                if let Ok(code) = tail[..end].parse() {
                    return code;
                }
            }
        }
    }
    0
}

fn tool_summary(name: &str, arguments: &str) -> String {
    let parsed: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    if name == "grep" {
        if let (Some(pattern), Some(path)) = (
            parsed.get("pattern").and_then(Value::as_str),
            parsed.get("path").and_then(Value::as_str),
        ) {
            let pattern = serde_json::to_string(pattern).unwrap_or_else(|_| "\"\"".into());
            let path = serde_json::to_string(path).unwrap_or_else(|_| "\"\"".into());
            return format!("{pattern} at {path}");
        }
    }
    if matches!(name, "bash" | "shell" | "powershell" | "pwsh") {
        for key in ["command", "cmd", "script"] {
            if let Some(value) = parsed.get(key).and_then(Value::as_str) {
                return value.lines().next().unwrap_or(value).to_owned();
            }
        }
    }
    let compact = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(120).collect()
}

fn classify_file_call(
    name: &str,
    arguments: &str,
    workspace: Option<&str>,
) -> Option<(FileAction, String)> {
    let parsed: Value = serde_json::from_str(arguments).ok()?;
    fn file_path(parsed: &Value) -> Option<&str> {
        parsed
            .get("path")
            .or_else(|| parsed.get("file_path"))
            .or_else(|| parsed.get("filePath"))
            .or_else(|| parsed.get("file"))
            .and_then(Value::as_str)
    }
    let (action, path) = match name {
        "read" | "read_text" | "read_image" => (FileAction::Read, file_path(&parsed)?),
        "edit" | "write" => (FileAction::Edit, file_path(&parsed)?),
        "str_replace_editor" => {
            let command = parsed.get("command")?.as_str()?;
            let action = match command {
                "view" => FileAction::View,
                "str_replace" => FileAction::Replace,
                "insert" => FileAction::Insert,
                "create" => FileAction::Create,
                _ => return None,
            };
            (action, file_path(&parsed)?)
        }
        _ => return None,
    };
    Some((action, workspace_relative_path(path, workspace)))
}

fn workspace_relative_path(path: &str, workspace: Option<&str>) -> String {
    let normalize = |value: &str| value.trim_end_matches(['/', '\\']).replace('\\', "/");
    let normalized_path = normalize(path);
    let Some(workspace) = workspace else {
        return normalized_path;
    };
    let normalized_workspace = normalize(workspace);
    let path_parts = normalized_path.split('/').collect::<Vec<_>>();
    let workspace_parts = normalized_workspace.split('/').collect::<Vec<_>>();
    let inside = path_parts.len() >= workspace_parts.len()
        && path_parts
            .iter()
            .zip(&workspace_parts)
            .all(|(left, right)| left.eq_ignore_ascii_case(right));
    if !inside {
        return normalized_path;
    }
    let relative = &path_parts[workspace_parts.len()..];
    if relative.is_empty() {
        ".".into()
    } else {
        relative.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(ty: &str, seq: u64, data: Value) -> HostEvent {
        HostEvent::from_value(serde_json::json!({
            "type": ty, "seq": seq, "time": seq * 10, "data": data
        }))
    }

    #[test]
    fn folds_file_actions_and_settles_one_public_row() {
        let mut state = ToolProjectionState::default();
        for (seq, call, name, args) in [
            (1, "r", "read", r#"{"path":"C:/work/a.rs"}"#),
            (
                2,
                "e",
                "str_replace_editor",
                r#"{"command":"str_replace","path":"C:/work/b.rs"}"#,
            ),
        ] {
            let event = event(
                "tool/call",
                seq,
                serde_json::json!({"callId": call, "name": name, "arguments": args}),
            );
            state.project_call(&event, Some("C:/work"), true, seq * 10);
        }
        let result = event(
            "tool/result",
            3,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "r",
                    "content": "ok"
                }]}
            }),
        );
        state.project_result(&result, 30);
        let result = event(
            "tool/result",
            4,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "e",
                    "content": "ok"
                }]}
            }),
        );
        let ToolMutation::Upsert(row) = state.project_result(&result, 40).unwrap() else {
            panic!("row")
        };
        assert_eq!(row.state, ActivityState::Success);
        assert_eq!(row.summary, "a.rs");
        assert_eq!(row.continuations[0].summary, "b.rs");
    }

    #[test]
    fn create_is_concise_and_has_no_duration_or_output_continuation() {
        let mut state = ToolProjectionState::default();
        let call = event(
            "tool/call",
            1,
            serde_json::json!({
                "callId":"c", "name":"str_replace_editor",
                "arguments": r#"{"command":"create","path":"C:/work/new.rs"}"#
            }),
        );
        state.project_call(&call, Some("C:/work"), true, 10);
        let result = event(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "c",
                    "content": "created"
                }]}
            }),
        );
        let ToolMutation::Upsert(row) = state.project_result(&result, 20).unwrap() else {
            panic!("row")
        };
        assert!(row.duration_ms.is_none());
        assert!(row.output_lines.is_none());
        assert!(row.continuations.is_empty());
    }

    #[test]
    fn grep_summary_reads_as_pattern_at_path() {
        assert_eq!(
            tool_summary("grep", r#"{"path":"README.md","pattern":"say \"hello\""}"#,),
            r#""say \"hello\"" at "README.md""#
        );
    }

    #[test]
    fn ask_user_question_is_owned_only_by_the_input_page() {
        let mut state = ToolProjectionState::default();
        let call = event(
            "tool/call",
            1,
            serde_json::json!({
                "callId":"question-call", "name":"ask_user_question",
                "arguments": r#"{"questions":[]}"#
            }),
        );
        assert_eq!(
            state.project_call(&call, Some("C:/work"), true, 10),
            Some(ToolMutation::Ignore)
        );
        assert!(state.row_for_call("question-call").is_none());

        let result = event(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "question-call",
                    "content": "answered"
                }]}
            }),
        );
        assert_eq!(
            state.project_result(&result, 20),
            Some(ToolMutation::Ignore)
        );
    }

    #[test]
    fn generic_tool_metrics_exist_while_running_and_settle_on_result() {
        let mut state = ToolProjectionState::default();
        let call = event(
            "tool/call",
            1,
            serde_json::json!({
                "callId":"p", "name":"pwsh",
                "arguments": r#"{"command":"cargo test"}"#
            }),
        );
        let ToolMutation::Upsert(running) = state
            .project_call(&call, Some("C:/work"), true, 10)
            .unwrap()
        else {
            panic!("row")
        };
        assert_eq!(running.output_lines, Some(0));
        assert!(running.live_duration_since.is_some());
        assert!(running.duration_ms.is_none());

        let result = event(
            "tool/result",
            2,
            serde_json::json!({
                "message": {"content": [{
                    "type": "tool-result",
                    "toolCallId": "p",
                    "content": [{"type": "text", "text": "one\ntwo"}]
                }]}
            }),
        );
        let ToolMutation::Upsert(done) = state.project_result(&result, 120).unwrap() else {
            panic!("row")
        };
        assert_eq!(done.output_lines, Some(2));
        assert_eq!(done.duration_ms, Some(110));
        assert!(done.live_duration_since.is_none());
    }
}
