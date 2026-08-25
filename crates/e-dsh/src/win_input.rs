//! Windows raw-byte terminal input source.
//!
//! crossterm's Windows event source reads console input records and can never
//! produce [`Event::Paste`] (see `vt_input`). Instead, this module enables
//! `ENABLE_VIRTUAL_TERMINAL_INPUT` on the console and reads the raw VT byte
//! stream from stdin, feeding it through [`VtInputParser`] so the rest of the
//! client receives normal crossterm events plus real paste events.
//!
//! Windows Terminal encodes Ctrl+Backspace as ETB (`0x17`) and Ctrl+H as BS
//! (`0x08`), so the reader runs on a dedicated blocking thread and forwards
//! byte chunks plus their immediate physical-key snapshot over a channel;
//! sequence reassembly, UTF-8 decoding, and the Escape-key timeout live in the
//! parser, which runs on the async main loop.

use std::io::Read;

use crossterm::event::Event;
use tokio::time::Instant;

use crate::vt_input::{NativeMods, Pending, VtInputParser, ESCAPE_TIMEOUT, SEQUENCE_TIMEOUT};

#[derive(Debug)]
struct InputChunk {
    bytes: Vec<u8>,
    mods: NativeMods,
}

pub struct WindowsRawInput {
    rx: tokio::sync::mpsc::UnboundedReceiver<InputChunk>,
    parser: VtInputParser,
    /// Kept on the source rather than inside one `next_event` future: the main
    /// loop selects this future against bridge/frame deadlines and may cancel
    /// it repeatedly. Recreating the timeout on every poll would starve a lone
    /// Escape forever while animation is active.
    pending_deadline: Option<(Pending, Instant)>,
}

impl WindowsRawInput {
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut reader = stdin.lock();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // Capture key state before the byte crosses into the
                        // async loop; Ctrl/Backspace may be released by then.
                        let chunk = InputChunk {
                            bytes: buf[..n].to_vec(),
                            mods: native_mods(),
                        };
                        if tx.send(chunk).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            rx,
            parser: VtInputParser::new(Box::new(native_mods)),
            pending_deadline: None,
        }
    }

    fn pending_timeout(&mut self) -> Option<Instant> {
        let pending = self.parser.pending();
        if pending == Pending::None {
            self.pending_deadline = None;
            return None;
        }
        if self
            .pending_deadline
            .map_or(true, |(armed, _)| armed != pending)
        {
            let delay = match pending {
                Pending::Escape => ESCAPE_TIMEOUT,
                Pending::Sequence => SEQUENCE_TIMEOUT,
                Pending::None => unreachable!("handled above"),
            };
            self.pending_deadline = Some((pending, Instant::now() + delay));
        }
        self.pending_deadline.map(|(_, deadline)| deadline)
    }

    /// Resolve the next terminal event, waiting for bytes or a timeout. The
    /// timeout state lives on `self`, making this method cancellation-safe.
    pub async fn next_event(&mut self) -> Option<Result<Event, String>> {
        loop {
            if let Some(event) = self.parser.pop() {
                return Some(Ok(event));
            }
            let deadline = self.pending_timeout();
            tokio::select! {
                chunk = self.rx.recv() => {
                    match chunk {
                        Some(chunk) => {
                            // Receiving another fragment restarts the timeout
                            // for the now-current partial sequence.
                            self.pending_deadline = None;
                            self.parser.feed_with_native_mods(&chunk.bytes, chunk.mods);
                        }
                        None => return None,
                    }
                }
                _ = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    self.pending_deadline = None;
                    self.parser.flush_timeout();
                }
            }
        }
    }
}

impl Default for WindowsRawInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Sample physical modifiers immediately after the blocking read. This
/// recovers Enter's unencoded modifiers and disambiguates raw Backspace before
/// the reader-to-async handoff can observe released keys.
fn native_mods() -> NativeMods {
    use winapi::um::winuser::{GetAsyncKeyState, VK_BACK, VK_CONTROL, VK_MENU, VK_SHIFT};
    fn down(key: i32) -> bool {
        unsafe { (GetAsyncKeyState(key) as u16) & 0x8000 != 0 }
    }
    NativeMods {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        back: down(VK_BACK),
    }
}

/// Enable `ENABLE_VIRTUAL_TERMINAL_INPUT` on the console input handle so the
/// terminal delivers the raw VT byte stream (pi does the same on Windows).
/// Must run after the complete ratatui/crossterm terminal construction, whose
/// raw-mode setup otherwise clears the flag again.
pub fn enable_virtual_terminal_input() -> std::io::Result<()> {
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::STD_INPUT_HANDLE;
    use winapi::um::wincon::ENABLE_VIRTUAL_TERMINAL_INPUT;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut mode: u32 = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    fn with_rx(rx: tokio::sync::mpsc::UnboundedReceiver<InputChunk>) -> WindowsRawInput {
        WindowsRawInput {
            rx,
            parser: VtInputParser::new(Box::new(|| NativeMods::default())),
            pending_deadline: None,
        }
    }

    #[tokio::test]
    async fn byte_chunks_become_events() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        tx.send(InputChunk {
            bytes: b"\x1b[200~a\r\nb\x1b[201~".to_vec(),
            mods: NativeMods::default(),
        })
        .unwrap();
        let event = input.next_event().await.unwrap().unwrap();
        assert_eq!(event, Event::Paste("a\nb".into()));
    }

    /// The real Windows Terminal Ctrl+Backspace encoding, end to end: ETB
    /// bytes plus the reader-time snapshot must delete a whole word.
    #[tokio::test]
    async fn reader_snapshot_preserves_ctrl_backspace_for_etb() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        tx.send(InputChunk {
            bytes: b"\x17".to_vec(),
            mods: NativeMods {
                ctrl: true,
                back: true,
                ..NativeMods::default()
            },
        })
        .unwrap();

        let event = input.next_event().await.unwrap().unwrap();
        assert_eq!(
            event,
            Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL))
        );

        let Event::Key(key) = event else {
            panic!("raw Ctrl+Backspace must become a key event");
        };
        let mut composer = e_tui::input::InputState::new(&e_tui::Config::default());
        for character in "hello world".chars() {
            composer.handle_key(
                &KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                true,
            );
        }
        composer.handle_key(&key, true);
        assert_eq!(composer.buf, "hello");
    }

    #[tokio::test]
    async fn escape_key_is_flushed_by_timeout() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        tx.send(InputChunk {
            bytes: b"\x1b".to_vec(),
            mods: NativeMods::default(),
        })
        .unwrap();
        let event = input.next_event().await.unwrap().unwrap();
        assert_eq!(
            event,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        );
    }

    #[tokio::test]
    async fn escape_timeout_survives_cancelled_next_event_futures() {
        use std::{
            future::Future,
            task::{Context, Poll},
        };

        use futures_util::task::noop_waker_ref;

        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        input.parser.feed(b"\x1b");

        // Poll once, then drop exactly as the outer runtime select does when a
        // frame or bridge branch wins.
        let mut first = Box::pin(input.next_event());
        let mut context = Context::from_waker(noop_waker_ref());
        assert!(matches!(first.as_mut().poll(&mut context), Poll::Pending));
        drop(first);
        let original = input.pending_deadline.expect("first poll arms Escape").1;

        let mut second = Box::pin(input.next_event());
        assert!(matches!(second.as_mut().poll(&mut context), Poll::Pending));
        drop(second);
        assert_eq!(
            input.pending_deadline.map(|(_, deadline)| deadline),
            Some(original),
            "a cancelled next_event future must not restart Escape timeout"
        );
    }

    #[tokio::test]
    async fn closed_channel_ends_the_stream() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        drop(tx);
        assert!(input.next_event().await.is_none());
    }

    #[tokio::test]
    async fn truncated_sequence_then_close_ends_stream() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut input = with_rx(rx);
        tx.send(InputChunk {
            bytes: b"\x1b[1;".to_vec(),
            mods: NativeMods::default(),
        })
        .unwrap();
        drop(tx);
        assert!(input.next_event().await.is_none());
    }
}
