//! smoke_snapshot — feed a captured snapshot file through the model fold and
//! report the resulting public display-surface mix. Usage:
//!   cargo run --example smoke_snapshot -- <path-to-snapshot.json>

use e::model::AppState;
use e_tui::{
    display::{ActivityState, CardRole, DisplayItem, TranscriptFormat},
    presentation::materialize_transcript,
    ui::provenance_layout_rows,
};

fn count_activity(
    state: ActivityState,
    tools: &mut usize,
    tools_ok: &mut usize,
    tools_fail: &mut usize,
    tools_running: &mut usize,
) {
    *tools += 1;
    match state {
        ActivityState::Waiting | ActivityState::Running => *tools_running += 1,
        ActivityState::Success => *tools_ok += 1,
        ActivityState::Failure | ActivityState::Cancelled => *tools_fail += 1,
    }
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../tools/cache/snapshot-sample.json".into());
    let raw = std::fs::read_to_string(&path)?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&raw)?;

    let mut state = AppState::default();
    for event in &events {
        state.apply_event(event);
    }
    materialize_transcript(&mut state);
    state.render.transcript_cache.width = 120;

    let mut users = 0;
    let mut assistants = 0;
    let mut systems = 0;
    let mut tools = 0;
    let mut tools_ok = 0;
    let mut tools_fail = 0;
    let mut tools_running = 0;
    for node in state.transcript.nodes() {
        match &node.item {
            DisplayItem::Card(card) if card.role == CardRole::User => users += 1,
            DisplayItem::Card(_) => systems += 1,
            DisplayItem::Block(block)
                if matches!(
                    block.format,
                    TranscriptFormat::Markdown | TranscriptFormat::Reasoning
                ) =>
            {
                assistants += 1;
            }
            DisplayItem::Block(_) => systems += 1,
            DisplayItem::Activity(row) if row.id.0.starts_with("thinking:") => {}
            DisplayItem::Activity(row) => count_activity(
                row.state,
                &mut tools,
                &mut tools_ok,
                &mut tools_fail,
                &mut tools_running,
            ),
            DisplayItem::Composite { activity, .. } => count_activity(
                activity.state,
                &mut tools,
                &mut tools_ok,
                &mut tools_fail,
                &mut tools_running,
            ),
        }
    }

    let provenance_rows = provenance_layout_rows(&state);
    let render_lines = provenance_rows.len();
    let atomic_rows = provenance_rows.iter().filter(|row| row.atomic).count();
    let units = provenance_rows
        .iter()
        .map(|row| row.unit)
        .collect::<std::collections::HashSet<_>>();

    println!("snapshot events: {}", events.len());
    println!("display nodes: {}", state.transcript.len());
    println!("  user:      {users}");
    println!(
        "  assistant: {assistants} (rendered {render_lines} provenance rows, {} units, {atomic_rows} atomic rows)",
        units.len()
    );
    println!("  tools:     {tools} (ok {tools_ok}, failed {tools_fail}, running {tools_running})");
    println!("  system:    {systems}");
    println!("OK");
    Ok(())
}
