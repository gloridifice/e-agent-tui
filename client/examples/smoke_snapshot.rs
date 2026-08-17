//! smoke_snapshot — feed a captured snapshot file through the model fold and
//! report the resulting message mix. Usage:
//!   cargo run --example smoke_snapshot -- <path-to-snapshot.json>

use e::model::{AppState, Msg, ToolState};

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

    let mut users = 0;
    let mut assistants = 0;
    let mut systems = 0;
    let mut tools = 0;
    let mut tools_ok = 0;
    let mut tools_fail = 0;
    let mut tools_running = 0;
    let mut render_lines = 0;
    let mut atomic_rows = 0;
    let mut units = std::collections::HashSet::new();
    for msg in &state.msgs {
        match msg {
            Msg::Card(card) if card.role == e::display::CardRole::User => users += 1,
            Msg::Card(_) => systems += 1,
            Msg::Block(block)
                if matches!(
                    block.format,
                    e::display::TranscriptFormat::Markdown
                        | e::display::TranscriptFormat::Reasoning
                ) =>
            {
                assistants += 1
            }
            Msg::Block(_) => systems += 1,
            Msg::Activity(row) => {
                tools += 1;
                match row.state {
                    e::display::ActivityState::Waiting | e::display::ActivityState::Running => {
                        tools_running += 1
                    }
                    e::display::ActivityState::Success => tools_ok += 1,
                    e::display::ActivityState::Failure | e::display::ActivityState::Cancelled => {
                        tools_fail += 1
                    }
                }
            }
            Msg::User { .. } => users += 1,
            Msg::Assistant { lines, .. } => {
                assistants += 1;
                render_lines += lines.len();
                atomic_rows += lines.iter().filter(|l| l.atomic).count();
                for line in lines {
                    units.insert(line.unit);
                }
            }
            Msg::Streaming { .. } => {}
            Msg::FileGroup(group) => {
                tools += 1;
                if group.pending() {
                    tools_running += 1;
                } else {
                    tools_ok += 1;
                }
            }
            Msg::Tool(card) => {
                tools += 1;
                match card.state {
                    ToolState::Running => tools_running += 1,
                    ToolState::Done { ok: true, .. } => tools_ok += 1,
                    ToolState::Done { ok: false, .. } => tools_fail += 1,
                }
            }
            Msg::Thinking(_) => {}
            Msg::System { .. } => systems += 1,
            Msg::Error { .. } => {}
        }
    }

    println!("snapshot events: {}", events.len());
    println!("folded messages: {}", state.msgs.len());
    println!("  user:      {users}");
    println!("  assistant: {assistants} (rendered {render_lines} lines, {} units, {atomic_rows} atomic rows)", units.len());
    println!("  tools:     {tools} (ok {tools_ok}, failed {tools_fail}, running {tools_running})");
    println!("  system:    {systems}");
    println!("OK");
    Ok(())
}
