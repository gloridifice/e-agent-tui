//! Tool and folded-file projection into public activity rows.

use std::collections::{HashMap, HashSet};

use crate::{
    agent::{
        timeline::{TimelineFact, TimelineRecord},
        tool::{ActivityState as AgentActivityState, ToolCapability, ToolReference},
    },
    display::{ActivityContinuation, ActivityRow, ActivityState, DisplayId},
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
        event: &TimelineRecord,
        session_cwd: Option<&str>,
        merge_files: bool,
        now_ms: u64,
    ) -> Option<ToolMutation> {
        let TimelineFact::ToolCall(activity) = &event.fact else {
            return None;
        };
        let call_id = &activity.id;
        if matches!(
            activity.capability,
            ToolCapability::Custom {
                ref namespace,
                ref name
            } if namespace == "interaction" && name == "question"
        ) {
            self.close_group();
            self.ignored_calls.insert(call_id.clone());
            return Some(ToolMutation::Ignore);
        }
        let file = classify_file_call(activity, session_cwd);
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
            .unwrap_or_else(|| (activity.label.clone(), activity.summary.clone(), false));
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

    pub fn project_result(&mut self, event: &TimelineRecord, now_ms: u64) -> Option<ToolMutation> {
        let TimelineFact::ToolResult {
            activity_id,
            output,
            state,
            output_truncated,
            ..
        } = &event.fact
        else {
            return None;
        };
        if self.ignored_calls.remove(activity_id) {
            return Some(ToolMutation::Ignore);
        }
        let Some(call) = self.calls.get(activity_id).cloned() else {
            return Some(ToolMutation::MissingResult);
        };
        let ok = matches!(state, AgentActivityState::Success);
        if let Some(group) = self.groups.get_mut(&call.row_id) {
            if let Some(item) = group
                .items
                .iter_mut()
                .find(|item| item.call_id == *activity_id)
            {
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

fn classify_file_call(
    activity: &crate::agent::tool::ToolActivity,
    workspace: Option<&str>,
) -> Option<(FileAction, String)> {
    let action = match activity.capability {
        ToolCapability::Read => FileAction::Read,
        ToolCapability::View => FileAction::View,
        ToolCapability::Edit => FileAction::Edit,
        ToolCapability::Insert => FileAction::Insert,
        ToolCapability::Replace => FileAction::Replace,
        ToolCapability::Create => FileAction::Create,
        _ => return None,
    };
    let path = match activity.reference.as_ref()? {
        ToolReference::Path { path } | ToolReference::Lines { path, .. } => path,
        ToolReference::Diff {
            path: Some(path), ..
        } => path,
        ToolReference::Hunks(hunks) => {
            let path = hunks.first()?.path.as_ref()?;
            if !hunks
                .iter()
                .all(|hunk| hunk.path.as_deref() == Some(path.as_str()))
            {
                return None;
            }
            path
        }
        _ => return None,
    };
    Some((
        action,
        crate::agent::tool::workspace_relative_path(path, workspace),
    ))
}

#[cfg(test)]
mod tests {
    use crate::{
        agent::tool::{ActivityState, ToolActivity},
        preview::MutationHunk,
    };

    use super::*;

    fn edit_with_hunks(paths: &[Option<&str>]) -> ToolActivity {
        ToolActivity {
            id: "edit-1".into(),
            capability: ToolCapability::Edit,
            label: "edit".into(),
            summary: "edit".into(),
            state: ActivityState::Running,
            reference: Some(ToolReference::Hunks(
                paths
                    .iter()
                    .map(|path| MutationHunk {
                        path: path.map(str::to_owned),
                        old: Some("old".into()),
                        new: Some("new".into()),
                        anchor_line: None,
                    })
                    .collect(),
            )),
            items: Vec::new(),
            preview: None,
        }
    }

    #[test]
    fn single_path_hunks_are_file_activities() {
        let activity = edit_with_hunks(&[Some(r"G:\repo\src\lib.rs"), Some(r"G:\repo\src\lib.rs")]);
        assert_eq!(
            classify_file_call(&activity, Some(r"G:\repo")),
            Some((FileAction::Edit, "src/lib.rs".into()))
        );
    }

    #[test]
    fn pathless_or_multi_path_hunks_are_not_one_file_activity() {
        assert!(classify_file_call(&edit_with_hunks(&[None]), None).is_none());
        assert!(
            classify_file_call(&edit_with_hunks(&[Some("a.rs"), Some("b.rs")]), None).is_none()
        );
    }
}
