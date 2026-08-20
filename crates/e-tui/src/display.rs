//! Owned presentation contracts shared by agent-event projection and the TUI.
//! Event reducers emit these models; ratatui rendering remains in `ui`.

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DisplayId(pub String);

impl DisplayId {
    pub fn event(seq: u64, role: &str) -> Self {
        Self(format!("event:{seq}:{role}"))
    }

    pub fn correlated(kind: &str, id: &str) -> Self {
        Self(format!("{kind}:{id}"))
    }

    pub fn legacy(index: usize, role: &str) -> Self {
        Self(format!("legacy:{index}:{role}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Waiting,
    Running,
    Success,
    Failure,
    Cancelled,
}

impl ActivityState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Waiting | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityContinuation {
    pub separator: String,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityRow {
    pub id: DisplayId,
    pub label: String,
    pub summary: String,
    pub continuations: Vec<ActivityContinuation>,
    pub state: ActivityState,
    pub start_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Output line count shown as trailing tool metadata from the moment a
    /// generic tool starts. `None` keeps non-tool activities concise.
    pub output_lines: Option<usize>,
    pub output_lines_truncated: bool,
    /// Monotonic start used only while a tool is running so elapsed metadata
    /// can update on animation patches without mutating the transcript model.
    pub live_duration_since: Option<std::time::Instant>,
    pub parent_id: Option<DisplayId>,
    pub depth: u16,
    pub count: usize,
}

impl ActivityRow {
    pub fn root(id: DisplayId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            summary: String::new(),
            continuations: Vec::new(),
            state: ActivityState::Running,
            start_ms: None,
            duration_ms: None,
            output_lines: None,
            output_lines_truncated: false,
            live_duration_since: None,
            parent_id: None,
            depth: 0,
            count: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    Plain,
    Markdown,
    Reasoning,
    UnknownFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayTone {
    Normal,
    Dim,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptBlock {
    pub id: DisplayId,
    pub unit: Option<u64>,
    pub content: String,
    pub format: TranscriptFormat,
    pub tone: DisplayTone,
    pub copy_source: String,
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardRole {
    User,
    Context,
    Detail,
    Attachment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentCard {
    pub id: DisplayId,
    pub unit: Option<u64>,
    pub header: Option<String>,
    pub content: String,
    pub role: CardRole,
    pub tone: DisplayTone,
    pub horizontal_padding: usize,
    pub copy_source: String,
}

/// One merged Thinking-phase node. The breathing indicator row (activity
/// semantics: `Thinking...`, `xN` counting, settle transition) and the
/// streamed reasoning content are a single transcript node, so every surface
/// agrees on what "thinking" is: the main transcript renders the indicator
/// in `compact` and the content in `lines`/`full`; Reading and Preview
/// always surface the accumulated reasoning content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinkingNode {
    /// Breathing indicator row (id `thinking:N`, label `Thinking...`).
    pub row: ActivityRow,
    /// Copy unit for the accumulated reasoning content.
    pub unit: Option<u64>,
    /// Streamed reasoning text.
    pub content: String,
    pub copy_source: String,
    /// Whether reasoning chunks are still streaming into this node.
    pub streaming: bool,
    /// Turn this node's reasoning belongs to. Live nodes created by
    /// `start_thinking` start with `None` and adopt the first reasoning
    /// chunk's turn; replay-created nodes carry it from creation, so
    /// reasoning from later turns never merges into an earlier turn's node.
    pub turn: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayItem {
    Activity(ActivityRow),
    Block(TranscriptBlock),
    Card(ContentCard),
    Composite {
        activity: ActivityRow,
        detail: ContentCard,
    },
    Thinking(ThinkingNode),
}

impl DisplayItem {
    pub fn id(&self) -> &DisplayId {
        match self {
            Self::Activity(row) => &row.id,
            Self::Block(block) => &block.id,
            Self::Card(card) => &card.id,
            Self::Composite { activity, .. } => &activity.id,
            Self::Thinking(node) => &node.row.id,
        }
    }

    pub fn is_activity(&self) -> bool {
        matches!(
            self,
            Self::Activity(_) | Self::Composite { .. } | Self::Thinking(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputAccessoryKind {
    Approval,
    Queue,
    Todo,
    Goal,
    Plan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputAccessory {
    pub kind: InputAccessoryKind,
    pub priority: u16,
    pub desired_rows: u16,
    pub minimum_rows: u16,
    pub blocking: bool,
    pub insertion_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessoryAllocation {
    pub kind: InputAccessoryKind,
    pub rows: u16,
    pub focused: bool,
}

/// Allocate the bounded strip above the editor. Blocking accessories retain
/// their minimum before informational rows; focus belongs to the highest
/// priority blocking accessory only.
pub fn allocate_accessories(
    accessories: &[InputAccessory],
    row_budget: u16,
) -> Vec<AccessoryAllocation> {
    let mut ranked = accessories.to_vec();
    ranked.sort_by_key(|item| (std::cmp::Reverse(item.priority), item.insertion_order));
    let focused_kind = ranked
        .iter()
        .find(|item| item.blocking)
        .map(|item| item.kind);
    let mut remaining = row_budget;
    let mut allocated = Vec::new();
    for item in ranked {
        if remaining == 0 {
            break;
        }
        let wanted = item.desired_rows.min(remaining);
        let rows = if wanted >= item.minimum_rows {
            wanted
        } else {
            0
        };
        if rows == 0 {
            continue;
        }
        remaining -= rows;
        allocated.push(AccessoryAllocation {
            kind: item.kind,
            rows,
            focused: Some(item.kind) == focused_kind,
        });
    }
    // Visual order is stable and independent of allocation priority.
    allocated.sort_by_key(|item| match item.kind {
        InputAccessoryKind::Approval => 0,
        InputAccessoryKind::Goal => 1,
        InputAccessoryKind::Plan => 2,
        InputAccessoryKind::Todo => 3,
        InputAccessoryKind::Queue => 4,
    });
    allocated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_contract_supports_stable_parented_lifecycle() {
        let parent = DisplayId::correlated("tool", "root");
        let mut child = ActivityRow::root(DisplayId::correlated("tool", "child"), "read");
        child.parent_id = Some(parent.clone());
        child.depth = 1;
        assert!(child.state.is_active());
        child.state = ActivityState::Success;
        child.duration_ms = Some(25);
        assert!(!child.state.is_active());
        assert_eq!(child.parent_id, Some(parent));
    }

    #[test]
    fn input_accessories_prioritize_one_blocking_focus_and_collapse_info() {
        let items = vec![
            InputAccessory {
                kind: InputAccessoryKind::Queue,
                priority: 10,
                desired_rows: 4,
                minimum_rows: 1,
                blocking: false,
                insertion_order: 2,
            },
            InputAccessory {
                kind: InputAccessoryKind::Approval,
                priority: 90,
                desired_rows: 3,
                minimum_rows: 3,
                blocking: true,
                insertion_order: 1,
            },
        ];
        let plan = allocate_accessories(&items, 4);
        assert_eq!(plan.iter().filter(|item| item.focused).count(), 1);
        assert!(plan
            .iter()
            .any(|item| item.kind == InputAccessoryKind::Approval && item.focused));
        assert!(plan
            .iter()
            .any(|item| item.kind == InputAccessoryKind::Queue && item.rows == 1));
        let small = allocate_accessories(&items, 3);
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].kind, InputAccessoryKind::Approval);
        assert!(small[0].focused);
    }

    #[test]
    fn rich_event_composes_existing_surfaces() {
        let activity = ActivityRow::root(DisplayId::correlated("compaction", "c1"), "compact");
        let detail = ContentCard {
            id: DisplayId::correlated("compaction-detail", "c1"),
            unit: None,
            header: Some("Summary".into()),
            content: "bounded".into(),
            role: CardRole::Detail,
            tone: DisplayTone::Dim,
            horizontal_padding: 2,
            copy_source: "bounded".into(),
        };
        let item = DisplayItem::Composite { activity, detail };
        assert!(item.is_activity());
    }
}
