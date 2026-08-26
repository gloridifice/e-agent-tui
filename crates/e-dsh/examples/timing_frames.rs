//! timing_frames — repeatable release-mode transcript frame benchmark.
//!
//! Exercises long history, wide viewports, continuous scroll, a streaming
//! tail, and an active breathing row. The backend emits real Crossterm ANSI
//! into memory so changed-cell and byte counts are measured without requiring
//! an interactive terminal.
//!
//! Usage: cargo run --release --example timing_frames

use std::time::Instant;

use e::{
    config::Config,
    model::{tick_spinners, AppState},
    profile::{percentile, CountingBackend, CountingWriter, IoCounters},
};
use e_tui::{
    display::{
        CardRole, ContentCard, DisplayId, DisplayItem, DisplayTone, TranscriptBlock,
        TranscriptFormat,
    },
    input::InputState,
    ui::{render, scroll_lines, RenderOverlays, ScrollState},
};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
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
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> Result<(), Self::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(Position::ORIGIN)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: self.size,
            pixels: Size::default(),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

fn fixture() -> (AppState, Config) {
    let config = Config::default();
    let mut state = AppState::default();
    state.config = config.clone();
    for index in 0..1_000usize {
        if index % 5 == 0 {
            let text = format!(
                "request {index}: {}",
                "中文 wrapped content ".repeat(4 + index % 3)
            );
            state.transcript.append(
                DisplayItem::Card(ContentCard {
                    id: DisplayId::correlated("benchmark-user", &index.to_string()),
                    unit: None,
                    header: None,
                    content: text.clone(),
                    role: CardRole::User,
                    tone: DisplayTone::Normal,
                    horizontal_padding: config.user_input_padding,
                    copy_source: text,
                }),
                None,
            );
        } else {
            let text = format!(
                "response {index}: {}",
                "rendering benchmark content with markdown-like text ".repeat(3 + index % 4)
            );
            state.transcript.append(
                DisplayItem::Block(TranscriptBlock {
                    id: DisplayId::correlated("benchmark-assistant", &index.to_string()),
                    unit: None,
                    content: text.clone(),
                    format: TranscriptFormat::Markdown,
                    tone: DisplayTone::Normal,
                    copy_source: text,
                    streaming: false,
                }),
                None,
            );
        }
    }
    state.start_thinking();
    let stream_id = DisplayId::correlated("benchmark-assistant", "stream");
    state.transcript.append(
        DisplayItem::Block(TranscriptBlock {
            id: stream_id,
            unit: None,
            content: "stream".into(),
            format: TranscriptFormat::Markdown,
            tone: DisplayTone::Normal,
            copy_source: "stream".into(),
            streaming: true,
        }),
        None,
    );
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
                toast: None,
                settings: None,
                login: None,
                approval: None,
                queue: &[],
                pane_resize: Default::default(),
            },
        );
    })?;

    let mut frame_ms = Vec::with_capacity(FRAMES);
    let mut cells = Vec::with_capacity(FRAMES);
    let mut bytes = Vec::with_capacity(FRAMES);
    state.render.transcript_cache.take_work_stats();
    state.preview.take_work_stats();
    let mut rebuilds = 0u64;
    let mut patches = 0u64;
    let mut preview_rebuilds = 0u64;
    let mut preview_patches = 0u64;
    let mut preview_rows = 0u64;
    let stream_id = DisplayId::correlated("benchmark-assistant", "stream");
    let mut reading_moves = 0u64;
    for frame_index in 0..FRAMES {
        if let Some(node) = state.transcript.get_mut(&stream_id) {
            if let DisplayItem::Block(block) = &mut node.item {
                let next = char::from(b'a' + (frame_index % 26) as u8);
                block.content.push(next);
                block.copy_source.push(next);
            }
        }
        state.transcript.touch(&stream_id);
        state.render.transcript_cache.mark_tail_dirty();
        tick_spinners(&mut state, Instant::now());
        let visible = height.saturating_sub(7) as usize;
        if frame_index == FRAMES / 2 {
            state.enter_reading(&input, &mut scroll, visible);
        } else if frame_index > FRAMES / 2 {
            let delta = if frame_index % 2 == 0 { -1 } else { 1 };
            reading_moves += u64::from(state.move_reading_block(delta, &mut scroll, visible));
        }
        let total = state.render.transcript_cache.display_len();
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
                    toast: None,
                    settings: None,
                    login: None,
                    approval: None,
                    queue: &[],
                    pane_resize: Default::default(),
                },
            );
        })?;
        frame_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        let io = counters.snapshot();
        let work = state.render.transcript_cache.take_work_stats();
        let preview_work = state.preview.take_work_stats();
        rebuilds += work.rebuilds;
        patches += work.patches;
        preview_rebuilds += preview_work.rebuilds;
        preview_patches += preview_work.patches;
        preview_rows += preview_work.materialized_rows;
        cells.push(io.changed_cells as f64);
        bytes.push(io.emitted_bytes as f64);
    }

    let p50 = percentile(&mut frame_ms.clone(), 0.50);
    let p95 = percentile(&mut frame_ms, 0.95);
    let cells_p95 = percentile(&mut cells, 0.95);
    let bytes_p95 = percentile(&mut bytes, 0.95);
    println!(
        "{width}x{height}: frames={FRAMES} p50={p50:.3}ms p95={p95:.3}ms cells_p95={cells_p95:.0} bytes_p95={bytes_p95:.0} cache_rebuilds={rebuilds} cache_patches={patches} preview_rebuilds={preview_rebuilds} preview_patches={preview_patches} preview_rows={preview_rows} reading_moves={reading_moves} cached_lines={}",
        state.render.transcript_cache.lines.len()
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
