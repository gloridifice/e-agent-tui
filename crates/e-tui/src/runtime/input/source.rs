//! Cancellation-safe production terminal event acquisition.
//!
//! Native Windows modifier sampling remains an executable-adapter shim; this
//! module owns the shared byte reader, parser, and timeout behavior.

use crossterm::event::Event;

#[cfg(windows)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::runtime::ports::TerminalEventPort;

#[cfg(not(windows))]
use crossterm::event::EventStream;
#[cfg(not(windows))]
use futures_util::StreamExt;

#[cfg(windows)]
use {
    super::vt::{NativeMods, Pending, VtInputParser, ESCAPE_TIMEOUT, SEQUENCE_TIMEOUT},
    std::{io::Read, time::Duration},
    tokio::time::Instant,
};

#[cfg(windows)]
const RESIZE_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const PASTE_SHORTCUT_POLL_INTERVAL: Duration = Duration::from_millis(8);
#[cfg(windows)]
const PASTE_SHORTCUT_FALLBACK_DELAY: Duration = Duration::from_millis(75);
#[cfg(windows)]
const PASTE_SHORTCUT_DEDUP_WINDOW: Duration = Duration::from_millis(250);

pub struct ProductionTerminalEvents {
    #[cfg(not(windows))]
    stream: EventStream,
    #[cfg(windows)]
    raw: WindowsRawInput,
}

impl ProductionTerminalEvents {
    #[cfg(not(windows))]
    pub fn new() -> Self {
        Self {
            stream: EventStream::new(),
        }
    }

    #[cfg(windows)]
    pub fn new(native_mods: fn() -> NativeMods) -> Self {
        Self {
            raw: WindowsRawInput::new(native_mods),
        }
    }
}

#[cfg(not(windows))]
impl Default for ProductionTerminalEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalEventPort for ProductionTerminalEvents {
    fn next_event(
        &mut self,
    ) -> impl std::future::Future<Output = Option<Result<Event, String>>> + Send {
        async move {
            #[cfg(windows)]
            {
                self.raw.next_event().await
            }
            #[cfg(not(windows))]
            {
                self.stream
                    .next()
                    .await
                    .map(|result| result.map_err(|error| error.to_string()))
            }
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct InputChunk {
    bytes: Vec<u8>,
    mods: NativeMods,
}

#[cfg(windows)]
struct PasteShortcutFallback {
    focused: bool,
    pending_deadline: Option<Instant>,
    last_terminal_delivery: Option<Instant>,
}

#[cfg(windows)]
impl PasteShortcutFallback {
    fn new() -> Self {
        Self {
            focused: true,
            pending_deadline: None,
            last_terminal_delivery: None,
        }
    }

    fn shortcut_pressed(&mut self, now: Instant) {
        let recently_delivered = self
            .last_terminal_delivery
            .is_some_and(|last| now <= last + PASTE_SHORTCUT_DEDUP_WINDOW);
        if self.focused && !recently_delivered {
            self.pending_deadline = Some(now + PASTE_SHORTCUT_FALLBACK_DELAY);
        }
    }

    fn raw_input(&mut self, chunk: &InputChunk, now: Instant) {
        let terminal_delivered_paste = chunk.mods.paste
            || chunk.bytes.contains(&0x16)
            || chunk
                .bytes
                .windows(b"\x1b[200~".len())
                .any(|window| window == b"\x1b[200~");
        if terminal_delivered_paste {
            self.last_terminal_delivery = Some(now);
        }
        // If bytes arrive after the physical shortcut, the terminal handled
        // it. This includes a bracketed-paste opener split across reads.
        if self.pending_deadline.is_some() {
            self.pending_deadline = None;
        }
    }

    fn observe_event(&mut self, event: &Event) {
        match event {
            Event::FocusLost => {
                self.focused = false;
                self.pending_deadline = None;
            }
            Event::FocusGained => self.focused = true,
            _ => {}
        }
    }
}

#[cfg(windows)]
struct WindowsRawInput {
    rx: tokio::sync::mpsc::UnboundedReceiver<InputChunk>,
    resize_rx: tokio::sync::watch::Receiver<(u16, u16)>,
    paste_shortcut_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    parser: VtInputParser,
    pending_deadline: Option<(Pending, Instant)>,
    paste_fallback: PasteShortcutFallback,
}

#[cfg(windows)]
fn changed_size(previous: &mut (u16, u16), current: (u16, u16)) -> Option<(u16, u16)> {
    if *previous == current {
        return None;
    }
    *previous = current;
    Some(current)
}

#[cfg(windows)]
fn spawn_paste_shortcut_watcher(
    native_mods: fn() -> NativeMods,
) -> tokio::sync::mpsc::UnboundedReceiver<()> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut was_down = false;
        loop {
            std::thread::sleep(PASTE_SHORTCUT_POLL_INTERVAL);
            if tx.is_closed() {
                break;
            }
            let down = native_mods().paste;
            if down && !was_down && tx.send(()).is_err() {
                break;
            }
            was_down = down;
        }
    });
    rx
}

#[cfg(windows)]
fn spawn_resize_watcher() -> tokio::sync::watch::Receiver<(u16, u16)> {
    let initial = crossterm::terminal::size().unwrap_or((0, 0));
    let (tx, rx) = tokio::sync::watch::channel(initial);
    std::thread::spawn(move || {
        let mut previous = initial;
        loop {
            std::thread::sleep(RESIZE_POLL_INTERVAL);
            if tx.is_closed() {
                break;
            }
            let Ok(current) = crossterm::terminal::size() else {
                continue;
            };
            if let Some(size) = changed_size(&mut previous, current) {
                if tx.send(size).is_err() {
                    break;
                }
            }
        }
    });
    rx
}

#[cfg(windows)]
impl WindowsRawInput {
    fn new(native_mods: fn() -> NativeMods) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut reader = stdin.lock();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
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
            resize_rx: spawn_resize_watcher(),
            paste_shortcut_rx: spawn_paste_shortcut_watcher(native_mods),
            parser: VtInputParser::new(Box::new(native_mods)),
            pending_deadline: None,
            paste_fallback: PasteShortcutFallback::new(),
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
            .is_none_or(|(armed, _)| armed != pending)
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

    async fn next_event(&mut self) -> Option<Result<Event, String>> {
        loop {
            if let Some(event) = self.parser.pop() {
                self.paste_fallback.observe_event(&event);
                return Some(Ok(event));
            }
            let deadline = self.pending_timeout();
            let paste_deadline = self.paste_fallback.pending_deadline;
            tokio::select! {
                chunk = self.rx.recv() => {
                    match chunk {
                        Some(chunk) => {
                            self.pending_deadline = None;
                            self.paste_fallback.raw_input(&chunk, Instant::now());
                            self.parser.feed_with_native_mods(&chunk.bytes, chunk.mods);
                        }
                        None => return None,
                    }
                }
                shortcut = self.paste_shortcut_rx.recv(), if !self.paste_shortcut_rx.is_closed() => {
                    if shortcut.is_some() {
                        self.paste_fallback.shortcut_pressed(Instant::now());
                    }
                }
                resized = self.resize_rx.changed() => {
                    if resized.is_err() {
                        return None;
                    }
                    let (columns, rows) = *self.resize_rx.borrow_and_update();
                    return Some(Ok(Event::Resize(columns, rows)));
                }
                _ = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                    self.pending_deadline = None;
                    self.parser.flush_timeout();
                }
                _ = tokio::time::sleep_until(paste_deadline.unwrap_or_else(Instant::now)), if paste_deadline.is_some() => {
                    self.paste_fallback.pending_deadline = None;
                    return Some(Ok(Event::Key(KeyEvent::new(
                        KeyCode::Char('v'),
                        KeyModifiers::CONTROL,
                    ))));
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn resize_watcher_emits_only_changed_dimensions() {
        let mut previous = (120, 40);
        assert_eq!(changed_size(&mut previous, (120, 40)), None);
        assert_eq!(changed_size(&mut previous, (100, 35)), Some((100, 35)));
        assert_eq!(previous, (100, 35));
    }

    #[test]
    fn physical_paste_arms_a_fallback_when_the_terminal_emits_nothing() {
        let now = Instant::now();
        let mut fallback = PasteShortcutFallback::new();
        fallback.shortcut_pressed(now);
        assert_eq!(
            fallback.pending_deadline,
            Some(now + PASTE_SHORTCUT_FALLBACK_DELAY)
        );
    }

    #[test]
    fn terminal_paste_delivery_cancels_and_deduplicates_the_fallback() {
        let now = Instant::now();
        let mut fallback = PasteShortcutFallback::new();
        fallback.shortcut_pressed(now);
        fallback.raw_input(
            &InputChunk {
                bytes: b"\x1b[200~text\x1b[201~".to_vec(),
                mods: NativeMods::default(),
            },
            now,
        );
        assert_eq!(fallback.pending_deadline, None);

        fallback.shortcut_pressed(now + Duration::from_millis(1));
        assert_eq!(fallback.pending_deadline, None);
    }

    #[test]
    fn unfocused_terminal_ignores_physical_paste_detection() {
        let mut fallback = PasteShortcutFallback::new();
        fallback.observe_event(&Event::FocusLost);
        fallback.shortcut_pressed(Instant::now());
        assert_eq!(fallback.pending_deadline, None);
    }
}
