//! Provider-neutral local shell identities and completions.

use std::collections::HashSet;

use crate::display::DisplayId;

pub const OUTPUT_CACHE_BYTES: usize = 256 * 1024;
pub const OUTPUT_CACHE_LINES: usize = 2_000;
pub const OUTPUT_DISPLAY_ROWS: usize = 64;

/// Retain whole trailing lines; an over-budget single line is discarded too.
pub fn bound_output(output: &mut String) -> bool {
    let mut start = output.len().saturating_sub(OUTPUT_CACHE_BYTES);
    if start > 0 && output.as_bytes()[start - 1] != b'\n' {
        start = output.as_bytes()[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(output.len(), |offset| start + offset + 1);
    }
    let retained = output[start..]
        .split_inclusive('\n')
        .rev()
        .take(OUTPUT_CACHE_LINES)
        .map(str::len)
        .sum::<usize>();
    start = output.len() - retained;
    if start == 0 {
        return false;
    }
    output.drain(..start);
    true
}

#[derive(Debug, Clone)]
pub struct ShellRequest {
    pub id: DisplayId,
    pub command: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ShellResult {
    pub output: String,
    pub output_truncated: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub cancelled: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct ShellState {
    pub active: Option<DisplayId>,
    pub draft_ids: HashSet<DisplayId>,
    pub unfolded: HashSet<DisplayId>,
    pub next_id: u64,
}
