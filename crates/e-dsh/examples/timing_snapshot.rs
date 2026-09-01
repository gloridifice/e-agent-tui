//! timing_snapshot — headless startup-profile of the client fold path.
//!
//! Loads a captured snapshot frame (JSON array of events, see
//! tools/dump-snapshot.mjs) and times each startup phase the way the live
//! client executes them: JSON parse → model fold → first-frame render.
//!
//! Usage: cargo run --release --example timing_snapshot -- <snapshot.json>

use std::time::Instant;

use e::protocol::HostEvent;
use e_tui::ui::{render, ScrollState};
use e_tui::{input::InputState, runtime::RuntimeState};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../tools/cache/snapshot-sample.json".into());
    let t0 = Instant::now();

    // Phase 1: read + parse the wire frame (what from_wire does per message).
    let raw = std::fs::read_to_string(&path)?;
    let t1 = Instant::now();
    let events: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
    let t2 = Instant::now();
    println!(
        "snapshot frame: {} bytes, {} events",
        raw.len(),
        events.len()
    );
    println!(
        "  read+parse:  {:.2} ms",
        t2.duration_since(t1).as_secs_f64() * 1000.0
    );

    // Phase 2: fold events into messages (state.apply("snapshot")).
    let mut state = RuntimeState::default();
    state.config = e::config::load();
    let records = events
        .into_iter()
        .map(HostEvent::from_value)
        .map(e::bridge::adapter::normalize_host_event)
        .collect::<Vec<_>>();
    state.apply_snapshot(&records, false);
    let t3 = Instant::now();
    println!(
        "  model fold:  {:.2} ms ({} display nodes, {} render units)",
        t3.duration_since(t2).as_secs_f64() * 1000.0,
        state.transcript.len(),
        state.render.units.len()
    );

    // Phase 3: first frame (render cache build + ratatui draw), like the
    // live client's initial `terminal.draw`.
    let mut scroll = ScrollState::default();
    let input = InputState::new(&state.config);
    let theme = state.theme();
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = ratatui::Terminal::new(backend)?;
    terminal.draw(|frame| {
        render(
            frame,
            &mut state,
            &input,
            &mut scroll,
            &theme,
            e_tui::ui::RenderOverlays {
                input_page: None,
                help_visible: false,
                toast: None,
                settings: None,
                login: None,
                approval: None,
                queue: &[],
                pane_resize: Default::default(),
            },
        );
    })?;
    let t4 = Instant::now();
    println!(
        "  first frame: {:.2} ms ({} cached lines)",
        t4.duration_since(t3).as_secs_f64() * 1000.0,
        state.render.transcript_cache.lines.len()
    );

    println!(
        "total:         {:.2} ms",
        t4.duration_since(t0).as_secs_f64() * 1000.0
    );
    Ok(())
}
