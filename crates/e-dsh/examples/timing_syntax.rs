//! Repeatable release-mode syntax highlighting and cached Preview benchmark.
//!
//! Usage: cargo run --release --example timing_syntax

use std::{collections::HashMap, time::Instant};

use e_tui::{
    input::InputState,
    preview::{PreviewContent, PreviewState},
    render::{render_markdown, RenderOptions},
    syntax::{highlight_source, SyntaxHint},
    ui::{render, RenderOverlays, ScrollState},
    Theme, TuiApp,
};
use ratatui::{backend::TestBackend, Terminal};

fn p95(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() - 1) as f64 * 0.95).round() as usize]
}

fn overlays() -> RenderOverlays<'static> {
    RenderOverlays {
        help_visible: false,
        help_scroll: None,
        toast: None,
        input_page: None,
        settings: None,
        login: None,
        approval: None,
        queue: &[],
        pane_resize: Default::default(),
    }
}

fn main() {
    let theme = Theme::ferra();
    let code = (0..120)
        .map(|index| format!("fn item_{index}() -> usize {{ {index} }}"))
        .collect::<Vec<_>>()
        .join("\n");

    let start = Instant::now();
    let cold = highlight_source(&code, SyntaxHint::Token("rust"), &theme.code);
    let cold_ms = start.elapsed().as_secs_f64() * 1_000.0;
    assert_eq!(cold.len(), 120);

    let mut state = TuiApp::default();
    state.config.resolved_theme = theme;
    state.preview.fullscreen = true;
    state.preview.state =
        PreviewState::Ready(PreviewContent::Markdown(format!("```rust\n{code}\n```")));
    let input = InputState::new(&state.config);
    let mut scroll = ScrollState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test terminal");
    terminal
        .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
        .expect("warm Preview frame");
    let _ = state.preview.take_work_stats();

    let mut frame_ms = Vec::with_capacity(200);
    for _ in 0..200 {
        let start = Instant::now();
        terminal
            .draw(|frame| render(frame, &mut state, &input, &mut scroll, &theme, overlays()))
            .expect("cached Preview frame");
        frame_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
    }
    let preview_work = state.preview.take_work_stats();
    assert_eq!(
        preview_work.layout_rebuilds, 0,
        "cached frames must not rebuild styled Preview rows"
    );
    let frame_p95 = p95(&mut frame_ms);

    let stream_start = Instant::now();
    for visible in 1..=120 {
        let source = format!(
            "```rust\n{}\n```",
            code.lines().take(visible).collect::<Vec<_>>().join("\n")
        );
        let mut units = HashMap::new();
        render_markdown(
            &source,
            &theme,
            &mut 0,
            &RenderOptions::default(),
            &mut units,
        );
    }
    let stream_ms = stream_start.elapsed().as_secs_f64() * 1_000.0;

    let mut diff = String::from(
        "diff --git a/main.rs b/main.rs\n--- a/main.rs\n+++ b/main.rs\n@@ -1,996 +1,996 @@\n",
    );
    for index in 0..996 {
        diff.push_str(&format!("-fn old_{index}() {{}}\n"));
    }
    for index in 0..996 {
        diff.push_str(&format!("+fn new_{index}() {{}}\n"));
    }
    let diff_start = Instant::now();
    let diff_lines = e_tui::ui::component::diff::unified(&diff, Some("main.rs"), &theme, 120);
    let diff_ms = diff_start.elapsed().as_secs_f64() * 1_000.0;
    assert_eq!(diff_lines.len(), 1_996);

    println!("syntax cold highlight: {cold_ms:.2} ms");
    println!("cached Preview frame P95: {frame_p95:.2} ms");
    println!("120 growing code-fence materializations: {stream_ms:.2} ms");
    println!("1,996-row bounded diff: {diff_ms:.2} ms");
    println!(
        "Preview frame redline: {}",
        if frame_p95 <= 30.0 { "PASS" } else { "FAIL" }
    );
}
