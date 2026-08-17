//! Single-owner terminal lifecycle and atomic frame submission.

use std::{
    io::{self, BufWriter, Stdout, Write},
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Position, Size},
    Frame, Terminal,
};

use crate::profile::{CountingBackend, CountingWriter, IoCounters, IoSnapshot};

const WRITER_CAPACITY: usize = 64 * 1024;

pub type TerminalWriter = CountingWriter<BufWriter<Stdout>>;
pub type TerminalBackend = CountingBackend<CrosstermBackend<TerminalWriter>>;

#[derive(Debug, Clone, Copy, Default)]
pub struct FrameTransaction {
    pub render: Duration,
    pub total: Duration,
    pub io: IoSnapshot,
}

fn sync_output_enabled(value: Option<&str>) -> bool {
    value != Some("1")
}

fn sync_output_enabled_from_env() -> bool {
    sync_output_enabled(std::env::var("DSHE_DISABLE_SYNC_OUTPUT").ok().as_deref())
}

fn begin_sync<W: Write>(writer: &mut W, enabled: bool) -> io::Result<()> {
    if enabled {
        execute!(writer, BeginSynchronizedUpdate)?;
    }
    Ok(())
}

fn leave_terminal_modes<W: Write>(writer: &mut W) -> io::Result<()> {
    execute!(
        writer,
        LeaveAlternateScreen,
        cursor::Show,
        DisableMouseCapture,
        crossterm::event::DisableBracketedPaste
    )
}

/// Always attempts the synchronized-output terminator. `first` wins over an
/// End/flush error so callers see the operation that originally failed.
fn finish_sync<W: Write, T>(writer: &mut W, enabled: bool, first: io::Result<T>) -> io::Result<T> {
    let end = if enabled {
        execute!(writer, EndSynchronizedUpdate)
    } else {
        writer.flush()
    };
    match (first, end) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

/// Owns every terminal state transition. `restore` is idempotent and Drop is
/// a final safety net for early returns from the async runtime.
pub struct TerminalOwner {
    terminal: Terminal<TerminalBackend>,
    counters: IoCounters,
    sync_output: bool,
    restored: bool,
}

impl TerminalOwner {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let counters = IoCounters::default();
        let writer = CountingWriter::new(
            BufWriter::with_capacity(WRITER_CAPACITY, std::io::stdout()),
            counters.clone(),
        );
        let crossterm = CrosstermBackend::new(writer);
        let mut backend = CountingBackend::new(crossterm, counters.clone());
        if let Err(error) = execute!(
            backend,
            crossterm::event::EnableBracketedPaste,
            EnableMouseCapture,
            EnterAlternateScreen,
            cursor::Hide
        ) {
            // `execute!` may have delivered a prefix of the setup commands
            // before its write/flush failed. Drop the failed buffer first (its
            // Drop may retry residual bytes), then recover through a fresh
            // handle so no late setup write can undo the rollback.
            drop(backend);
            let mut stdout = std::io::stdout();
            let _ = leave_terminal_modes(&mut stdout);
            let _ = disable_raw_mode();
            return Err(error);
        }
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                // Terminal construction failed after entering the alternate
                // screen. Use a fresh stdout handle for best-effort recovery.
                let mut stdout = std::io::stdout();
                let _ = leave_terminal_modes(&mut stdout);
                let _ = disable_raw_mode();
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            counters,
            sync_output: sync_output_enabled_from_env(),
            restored: false,
        })
    }

    pub fn size(&self) -> io::Result<Size> {
        self.terminal.size()
    }

    pub fn draw<F>(&mut self, render: F) -> io::Result<FrameTransaction>
    where
        F: FnOnce(&mut Frame<'_>) -> Option<Position>,
    {
        let _zone = crate::tracy_zone!("frame transaction");
        let started = Instant::now();
        self.counters.reset();
        let begin = begin_sync(self.terminal.backend_mut(), self.sync_output);
        if let Err(error) = begin {
            // Continue to finish_sync so a partial successful Begin is always
            // paired with End before returning the write failure.
            return finish_sync(self.terminal.backend_mut(), self.sync_output, Err(error));
        }

        let mut cursor_anchor = None;
        let mut render_elapsed = Duration::ZERO;
        let draw_result = self
            .terminal
            .draw(|frame| {
                let render_started = Instant::now();
                cursor_anchor = render(frame);
                render_elapsed = render_started.elapsed();
            })
            .map(|_| ());
        let frame_result = draw_result.and_then(|()| {
            if let Some(position) = cursor_anchor {
                self.terminal.set_cursor_position(position)
            } else {
                Ok(())
            }
        });
        finish_sync(self.terminal.backend_mut(), self.sync_output, frame_result)?;
        Ok(FrameTransaction {
            render: render_elapsed,
            total: started.elapsed(),
            io: self.counters.snapshot(),
        })
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        let backend = self.terminal.backend_mut();
        // End first in case a failed frame left the emulator synchronized.
        let sync_result = if self.sync_output {
            execute!(backend, EndSynchronizedUpdate)
        } else {
            Ok(())
        };
        let command_result = leave_terminal_modes(backend);
        let raw_result = disable_raw_mode();
        sync_result.and(command_result).and(raw_result)
    }
}

impl Drop for TerminalOwner {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_commands_bracket_output() {
        let mut out = Vec::new();
        begin_sync(&mut out, true).unwrap();
        out.write_all(b"frame").unwrap();
        finish_sync(&mut out, true, Ok(())).unwrap();
        assert_eq!(out, b"\x1b[?2026hframe\x1b[?2026l");
    }

    #[test]
    fn disabled_sync_writes_only_frame_data() {
        let mut out = Vec::new();
        begin_sync(&mut out, false).unwrap();
        out.write_all(b"frame").unwrap();
        finish_sync(&mut out, false, Ok(())).unwrap();
        assert_eq!(out, b"frame");
    }

    #[test]
    fn finish_sync_emits_end_but_preserves_first_error() {
        let mut out = Vec::new();
        begin_sync(&mut out, true).unwrap();
        let error =
            finish_sync::<_, ()>(&mut out, true, Err(io::Error::other("draw failed"))).unwrap_err();
        assert_eq!(error.to_string(), "draw failed");
        assert!(out.ends_with(b"\x1b[?2026l"));
    }

    #[test]
    fn sync_disable_contract_uses_exact_one() {
        assert!(sync_output_enabled(None));
        assert!(sync_output_enabled(Some("0")));
        assert!(!sync_output_enabled(Some("1")));
    }
}
