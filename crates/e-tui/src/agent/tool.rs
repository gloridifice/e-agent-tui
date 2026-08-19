//! Kernel-neutral tool presentation facts.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolCapability {
    Read,
    View,
    Edit,
    Insert,
    Replace,
    Search,
    Command,
    Create,
    Generic,
    Custom { namespace: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Waiting,
    Running,
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolReference {
    Path {
        path: String,
    },
    Link {
        label: Option<String>,
        url: String,
    },
    Text {
        text: String,
    },
    Custom {
        namespace: String,
        kind: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolItem {
    pub id: String,
    pub label: String,
    pub reference: ToolReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivity {
    pub id: String,
    pub capability: ToolCapability,
    pub label: String,
    pub summary: String,
    pub state: ActivityState,
    pub reference: Option<ToolReference>,
    pub items: Vec<ToolItem>,
}
