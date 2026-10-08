//! Provider-neutral local shell identities and completions.

use std::collections::HashSet;

use crate::display::DisplayId;

#[derive(Debug, Clone)]
pub struct ShellRequest {
    pub id: DisplayId,
    pub command: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ShellResult {
    pub output: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub cancelled: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct ShellState {
    pub active: Option<DisplayId>,
    pub draft_ids: HashSet<DisplayId>,
    pub next_id: u64,
}
