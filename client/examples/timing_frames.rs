//! timing_frames — repeatable release-mode transcript frame benchmark.
//!
//! Exercises long history, wide viewports, continuous scroll, a streaming
//! tail, and an active breathing row. The backend emits real Crossterm ANSI
//! into memory so changed-cell and byte counts are measured without requiring
//! an interactive terminal.
//!
//! Usage: cargo run --release --example timing_frames

use std::{io, time::Instant};

use e::{
    config::Config,
    input::InputState,
    model::{tick_spinners, AppState, Msg},
    profile::{percentile, CountingBackend, CountingWriter, IoCounters},
    render::RenderLine,
    ui::{render, scroll_lines, RenderOverlays, ScrollState},
};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    text::Line,
    Terminal,
};

const FRAMES: usize = 120;

struct FixedBackend<B> {
    inner: B,
    size: Size,
}

impl<B> FixedBackend<B> {
    fn new(inner: B, width: u16, height: u16) -> Self {
        Self {
            inner,
            size: Size::new(width, height),
        }
    }
}

impl<B: Backend> Backend for FixedBackend<B> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::ORIGIN)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::default(),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn fixture() -> (AppState, Config) {
    let config = Config::default();
    let mut state = AppState::default();
    state.config = config.clone();
    for index in 0..1_000usize {
        if index % 5 == 0 {
            state.msgs.push(Msg::User {
                text: format!(
                    "request {index}: {}",
                    "中文 wrapped content ".repeat(4 + index % 3)
                ),
            });
        } else {
            let text = format!(
                "response {index}: {}",
                "rendering benchmark content with markdown-like text ".repeat(3 + index % 4)
            );
            state.msgs.push(Msg::Assistant {
                text: text.clone(),
                lines: vec![RenderLine {
                    line: Line::from(text),
                    unit: index as u64 + 1,
                    raw_line: Some(0),
                    atomic: false,
                    fill: false,
                }],
                unit_start: index as u64 + 1,
            });
        }
    }
    state.start_thinking();
    state.msgs.push(Msg::Streaming {
        text: "stream".into(),
    });
    (state, config)
}

fn run(width: u16, height: u16) -> anyhow::Result<()> {
    let (mut state, config) = fixture();
    let input = InputState::new(&config);
    let theme = state.theme();
    let counters = IoCounters::default();
    let writer = CountingWriter::new(Vec::<u8>::new(), counters.clone());
    let ansi = CrosstermBackend::new(writer);
    let fixed = FixedBackend::new(ansi, width, height);
    let backend = CountingBackend::new(fixed, counters.clone());
    let mut terminal = Terminal::new(backend)?;
    let mut scroll = ScrollState::default();

    terminal.draw(|frame| {
        render(
            frame,
            &mut state,
            &input,
            &mut scroll,
            &theme,
            RenderOverlays {
                input_page: None,
                help_visible: false,
                overlay: None,
                toast: None,
                settings: None,
                login: None,
            },
        );
    })?;

    let mut frame_ms = Vec::with_capacity(FRAMES);
    let mut cells = Vec::with_capacity(FRAMES);
    let mut bytes = Vec::with_capacity(FRAMES);
    state.transcript_cache.take_work_stats();
    let mut rebuilds = 0u64;
    let mut patches = 0u64;
    for frame_index in 0..FRAMES {
        if let Some(Msg::Streaming { text }) = state.msgs.last_mut() {
            text.push(char::from(b'a' + (frame_index % 26) as u8));
            state.transcript_cache.mark_tail_dirty();
        }
        tick_spinners(&mut state, Instant::now());
        let visible = height.saturating_sub(7) as usize;
        let total = state.transcript_cache.display_len();
        scroll_lines(&mut scroll, visible, total, frame_index % 40 < 20, 3);
        counters.reset();
        let started = Instant::now();
        terminal.draw(|frame| {
            render(
                frame,
                &mut state,
                &input,
                &mut scroll,
                &theme,
                RenderOverlays {
                    input_page: None,
                    help_visible: false,
                    overlay: None,
                    toast: None,
                    settings: None,
                    login: None,
                },
            );
        })?;
        frame_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let io = counters.snapshot();
        let work = state.transcript_cache.take_work_stats();
        rebuilds += work.rebuilds;
        patches += work.patches;
        cells.push(io.changed_cells as f64);
        bytes.push(io.emitted_bytes as f64);
    }

    let p50 = percentile(&mut frame_ms.clone(), 0.50);
    let p95 = percentile(&mut frame_ms, 0.95);
    let cells_p95 = percentile(&mut cells, 0.95);
    let bytes_p95 = percentile(&mut bytes, 0.95);
    println!(
        "{width}x{height}: frames={FRAMES} p50={p50:.3}ms p95={p95:.3}ms cells_p95={cells_p95:.0} bytes_p95={bytes_p95:.0} cache_rebuilds={rebuilds} cache_patches={patches} cached_lines={}",
        state.transcript_cache.lines.len()
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    println!("timing_frames release fixture: messages=1002 frames={FRAMES}");
    for (width, height) in [(80, 40), (160, 50), (240, 70)] {
        run(width, height)?;
    }
    Ok(())
}
