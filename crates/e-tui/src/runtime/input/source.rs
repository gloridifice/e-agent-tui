//! Cancellation-safe production terminal event acquisition.
//!
//! Native Windows modifier sampling remains an executable-adapter shim; this
//! module owns the shared byte reader, parser, and timeout behavior.

use crossterm::event::Event;

use crate::runtime::ports::TerminalEventPort;

#[cfg(not(windows))]
use crossterm::event::EventStream;
#[cfg(not(windows))]
use futures_util::StreamExt;

#[cfg(windows)]
use {
    super::vt::{NativeMods, Pending, VtInputParser, ESCAPE_TIMEOUT, SEQUENCE_TIMEOUT},
    std::io::Read,
    tokio::time::Instant,
};

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
struct WindowsRawInput {
    rx: tokio::sync::mpsc::UnboundedReceiver<InputChunk>,
    parser: VtInputParser,
    pending_deadline: Option<(Pending, Instant)>,
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
                return Some(Ok(event));
            }
            let deadline = self.pending_timeout();
            tokio::select! {
                chunk = self.rx.recv() => {
                    match chunk {
                        Some(chunk) => {
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
