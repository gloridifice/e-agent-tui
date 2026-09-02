//! Byte-stream terminal input parser for the Windows raw-input path.
//!
//! crossterm's Windows backend reads console input records
//! (`ReadConsoleInputW`) and never produces [`Event::Paste`]: the console
//! consumes the bracketed-paste wrapper (`ESC[200~ … ESC[201~`) before records
//! reach the application, so a paste's `\r` line endings arrive as plain Enter
//! key events and the composer sends the message at every newline.
//!
//! With `ENABLE_VIRTUAL_TERMINAL_INPUT` set on the console input handle, the
//! terminal instead delivers the raw VT byte stream to byte readers — the same
//! stream Unix terminals produce. This module parses that stream into crossterm
//! [`Event`]s, including [`Event::Paste`], following the proven design of the
//! pi terminal client: byte-level sequence buffering, plus reader-time
//! physical-key snapshots on Windows so Shift/Ctrl+Enter and Ctrl+Backspace
//! survive even though legacy raw bytes do not encode them unambiguously.
//!
//! The parser is a pure state machine over bytes: feed it chunks (reads may
//! split sequences arbitrarily) and drain [`Event`]s. Incomplete escape
//! sequences are held until more bytes arrive or a timeout flush decides what
//! to do with them.

use std::{collections::VecDeque, time::Duration};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

/// Native terminal context sampled by the Windows raw-input source.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeMods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Physical Ctrl+V state. Some Windows terminal hosts consume the shortcut
    /// without emitting bytes when the clipboard contains only an image, so
    /// the raw-input source uses this to synthesize the missing key event.
    pub paste: bool,
    /// Physical Backspace state captured when the reader receives the byte.
    /// Windows Terminal encodes Ctrl+Backspace as ETB (`0x17`) and Ctrl+H as
    /// BS (`0x08`), so this snapshot is what separates a real Backspace origin
    /// from the plain control character before the async handoff.
    pub back: bool,
}

/// How long to wait after a lone `ESC` before treating it as the Escape key
/// (the terminal encodes Alt+key as `ESC` followed by the key, so a lone `ESC`
/// is ambiguous until more bytes arrive).
pub const ESCAPE_TIMEOUT: Duration = Duration::from_millis(25);
/// How long to wait for the rest of an incomplete CSI sequence before dropping
/// it (a truncated sequence is never a key the client binds).
pub const SEQUENCE_TIMEOUT: Duration = Duration::from_millis(50);

/// Pending-input wait classification, used by the async reader to decide
/// whether to arm a timeout while waiting for more bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pending {
    None,
    /// The buffer is exactly `ESC`: flush as Escape after `ESCAPE_TIMEOUT`.
    Escape,
    /// The buffer holds an incomplete escape sequence: drop it after
    /// `SEQUENCE_TIMEOUT`.
    Sequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeOutcome {
    Emitted,
    Pending,
    Dropped,
}

pub struct VtInputParser {
    buf: Vec<u8>,
    in_paste: bool,
    out: VecDeque<Event>,
    native_mods: Box<dyn Fn() -> NativeMods + Send>,
    /// Snapshot attached to the byte chunk currently being drained. Production
    /// input supplies it from the blocking reader thread; the callback remains
    /// as a deterministic fallback for parser tests and timeout paths.
    feed_mods: Option<NativeMods>,
}

impl VtInputParser {
    pub fn new(native_mods: Box<dyn Fn() -> NativeMods + Send>) -> Self {
        Self {
            buf: Vec::new(),
            in_paste: false,
            out: VecDeque::new(),
            native_mods,
            feed_mods: None,
        }
    }

    /// Feed a raw byte chunk; reads may split sequences anywhere.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.feed_inner(bytes, None);
    }

    /// Feed bytes together with the physical state captured by the blocking
    /// reader immediately after the read completed.
    pub fn feed_with_native_mods(&mut self, bytes: &[u8], mods: NativeMods) {
        self.feed_inner(bytes, Some(mods));
    }

    fn feed_inner(&mut self, bytes: &[u8], mods: Option<NativeMods>) {
        self.feed_mods = mods;
        self.buf.extend_from_slice(bytes);
        self.drain();
        self.feed_mods = None;
    }

    fn current_native_mods(&self) -> NativeMods {
        self.feed_mods.unwrap_or_else(|| (self.native_mods)())
    }

    /// The physical snapshot is sampled after the read returns, so `back`
    /// reflects the most recently pressed key. It is only trustworthy for the
    /// final byte of the current chunk: a coalesced chunk (e.g. a plain
    /// Backspace followed by Ctrl+Backspace in one read) would otherwise let
    /// the later key's state promote the earlier plain Backspace. Control
    /// bytes never wait across feeds, so `buf.len() == 1` means "last byte".
    fn back_trusted(&self, mods: NativeMods) -> bool {
        mods.back && self.buf.len() == 1
    }

    /// Take the next parsed event, if any.
    pub fn pop(&mut self) -> Option<Event> {
        self.out.pop_front()
    }

    /// Whether the reader must arm a timeout while waiting for more bytes.
    pub fn pending(&self) -> Pending {
        if self.in_paste {
            return Pending::None;
        }
        if self.buf == b"\x1b" {
            return Pending::Escape;
        }
        if self.buf.first() == Some(&0x1b) {
            return Pending::Sequence;
        }
        Pending::None
    }

    /// Apply the timeout decision: a lone `ESC` becomes Escape; an incomplete
    /// sequence is dropped (it can never be a key the client binds).
    pub fn flush_timeout(&mut self) {
        match self.pending() {
            Pending::Escape => {
                self.buf.clear();
                self.out
                    .push_back(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
            }
            Pending::Sequence => self.buf.clear(),
            Pending::None => {}
        }
    }

    fn drain(&mut self) {
        loop {
            if self.in_paste {
                if let Some(index) = find_subslice(&self.buf, b"\x1b[201~") {
                    let content = self.buf[..index].to_vec();
                    self.buf.drain(..index + 6);
                    self.in_paste = false;
                    self.out.push_back(Event::Paste(normalize_paste(&content)));
                    continue;
                }
                return;
            }
            if self.buf.is_empty() {
                return;
            }
            if self.buf.starts_with(b"\x1b[200~") {
                self.buf.drain(..6);
                self.in_paste = true;
                continue;
            }
            let byte = self.buf[0];
            if byte == 0x1b {
                match self.try_escape() {
                    EscapeOutcome::Emitted | EscapeOutcome::Dropped => continue,
                    EscapeOutcome::Pending => return,
                }
            } else if byte < 0x20 {
                self.consume_control(byte);
                continue;
            } else if byte == 0x7f {
                // DEL is normally plain Backspace, but terminals differ on
                // Ctrl+Backspace. Trust only the physical state attached by
                // the reader thread, never a delayed async-loop sample, and
                // only for the final byte of the chunk.
                let mods = self.current_native_mods();
                let modifiers = if mods.ctrl && self.back_trusted(mods) {
                    KeyModifiers::CONTROL
                } else {
                    KeyModifiers::NONE
                };
                self.out
                    .push_back(Event::Key(KeyEvent::new(KeyCode::Backspace, modifiers)));
                self.buf.remove(0);
                continue;
            } else {
                if !self.consume_printable() {
                    // Incomplete UTF-8 tail: wait for the next chunk.
                    return;
                }
                continue;
            }
        }
    }

    fn consume_control(&mut self, byte: u8) {
        let event = match byte {
            // Windows terminals send Enter as CR; some terminals use LF.
            0x0d | 0x0a => self.enter_event(),
            0x09 => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            // Raw BS is Ctrl+H on this terminal family. Only an explicit
            // physical Backspace snapshot turns it into Ctrl+Backspace, which
            // covers terminals that do encode Ctrl+Backspace as BS.
            0x08 => {
                let mods = self.current_native_mods();
                if mods.ctrl && self.back_trusted(mods) {
                    KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL)
                } else if mods.ctrl {
                    KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)
                } else {
                    KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
                }
            }
            // Windows Terminal sends ETB for Ctrl+Backspace, matching the Unix
            // Ctrl+W "delete previous word" convention. A physical Backspace
            // snapshot identifies the Ctrl+Backspace origin; otherwise this
            // stays Ctrl+W, which the composer also treats as delete-word.
            0x17 => {
                let mods = self.current_native_mods();
                if self.back_trusted(mods) {
                    KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL)
                } else {
                    KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
                }
            }
            0x00 => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL),
            0x1c => KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::CONTROL),
            0x1d => KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
            0x1e => KeyEvent::new(KeyCode::Char('^'), KeyModifiers::CONTROL),
            0x1f => KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL),
            0x01..=0x1a => {
                KeyEvent::new(KeyCode::Char((byte + 0x60) as char), KeyModifiers::CONTROL)
            }
            _ => {
                self.buf.remove(0);
                return;
            }
        };
        self.out.push_back(Event::Key(event));
        self.buf.remove(0);
    }

    /// Enter can carry Shift/Ctrl/Alt even though the terminal only sends `\r`:
    /// sample the physical modifier state (pi's Windows heuristic).
    fn enter_event(&mut self) -> KeyEvent {
        let mods = self.current_native_mods();
        let modifiers = if mods.shift {
            KeyModifiers::SHIFT
        } else if mods.ctrl {
            KeyModifiers::CONTROL
        } else if mods.alt {
            KeyModifiers::ALT
        } else {
            KeyModifiers::NONE
        };
        KeyEvent::new(KeyCode::Enter, modifiers)
    }

    /// Consume one printable char (UTF-8). Returns false when the run ends
    /// with an incomplete multi-byte sequence that needs more bytes.
    fn consume_printable(&mut self) -> bool {
        let run_end = self
            .buf
            .iter()
            .position(|&b| b < 0x20 || b == 0x7f)
            .unwrap_or(self.buf.len());
        let run = &self.buf[..run_end];
        match std::str::from_utf8(run) {
            Ok(text) => {
                for c in text.chars() {
                    self.out.push_back(Event::Key(KeyEvent::new(
                        KeyCode::Char(c),
                        KeyModifiers::NONE,
                    )));
                }
                self.buf.drain(..run_end);
                true
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&run[..valid]).expect("validated prefix");
                    for c in text.chars() {
                        self.out.push_back(Event::Key(KeyEvent::new(
                            KeyCode::Char(c),
                            KeyModifiers::NONE,
                        )));
                    }
                    self.buf.drain(..valid);
                    true
                } else if error.error_len().is_none() {
                    // Incomplete trailing multi-byte char: hold for more bytes.
                    false
                } else {
                    // Invalid byte: drop it and continue.
                    self.buf.remove(0);
                    true
                }
            }
        }
    }

    fn try_escape(&mut self) -> EscapeOutcome {
        if self.buf.len() == 1 {
            return EscapeOutcome::Pending;
        }
        match self.buf[1] {
            b'[' => self.try_csi(),
            b'O' => self.try_ss3(),
            other => {
                // Alt+key arrives as ESC followed by the key byte.
                self.emit_alt(other);
                self.buf.drain(..2);
                EscapeOutcome::Emitted
            }
        }
    }

    fn try_csi(&mut self) -> EscapeOutcome {
        let rest = &self.buf[2..];
        let Some(final_index) = rest.iter().position(|&b| (0x40..=0x7e).contains(&b)) else {
            return EscapeOutcome::Pending;
        };
        let params = rest[..final_index].to_vec();
        let final_byte = rest[final_index];
        let total = 2 + final_index + 1;
        self.buf.drain(..total);
        if self.consume_csi(&params, final_byte) {
            EscapeOutcome::Emitted
        } else {
            EscapeOutcome::Dropped
        }
    }

    fn consume_csi(&mut self, params: &[u8], final_byte: u8) -> bool {
        let params = std::str::from_utf8(params).unwrap_or_default();
        match final_byte {
            b'A' | b'B' | b'C' | b'D' => {
                let code = match final_byte {
                    b'A' => KeyCode::Up,
                    b'B' => KeyCode::Down,
                    b'C' => KeyCode::Right,
                    _ => KeyCode::Left,
                };
                self.push_key(code, self.modifiers_from_csi(params));
                true
            }
            b'H' | b'F' => {
                let code = if final_byte == b'H' {
                    KeyCode::Home
                } else {
                    KeyCode::End
                };
                self.push_key(code, self.modifiers_from_csi(params));
                true
            }
            b'Z' => {
                self.push_key(KeyCode::Tab, KeyModifiers::SHIFT);
                true
            }
            b'I' if params.is_empty() => {
                self.out.push_back(Event::FocusGained);
                true
            }
            b'O' if params.is_empty() => {
                self.out.push_back(Event::FocusLost);
                true
            }
            b'~' => self.consume_csi_tilde(params),
            b'M' | b'm' => self.consume_sgr_mouse(params, final_byte),
            b'u' => self.consume_csi_u(params),
            _ => false,
        }
    }

    fn consume_csi_tilde(&mut self, params: &str) -> bool {
        let parts: Vec<&str> = params.split(';').collect();
        let number = parts.first().copied().unwrap_or("");
        let modifiers = self.modifiers_from_csi(params);
        match number {
            "1" | "7" => {
                self.push_key(KeyCode::Home, modifiers);
                true
            }
            "2" => {
                self.push_key(KeyCode::Insert, modifiers);
                true
            }
            "3" => {
                self.push_key(KeyCode::Delete, modifiers);
                true
            }
            "4" | "8" => {
                self.push_key(KeyCode::End, modifiers);
                true
            }
            "5" => {
                self.push_key(KeyCode::PageUp, modifiers);
                true
            }
            "6" => {
                self.push_key(KeyCode::PageDown, modifiers);
                true
            }
            // xterm modifyOtherKeys: ESC[27;<mod>;<code>~
            "27" if parts.len() >= 3 => {
                let code = parts[2].parse::<u32>().ok();
                match code {
                    Some(27) => {
                        self.push_key(KeyCode::Esc, modifiers);
                        true
                    }
                    Some(13) => {
                        self.push_key(KeyCode::Enter, modifiers);
                        true
                    }
                    Some(9) => {
                        self.push_key(KeyCode::Tab, modifiers);
                        true
                    }
                    Some(32) => {
                        self.push_key(KeyCode::Char(' '), modifiers);
                        true
                    }
                    Some(127) | Some(8) => {
                        self.push_key(KeyCode::Backspace, modifiers);
                        true
                    }
                    Some(code) if (32..=126).contains(&code) => {
                        self.push_key(
                            KeyCode::Char(char::from_u32(code).unwrap_or('?')),
                            modifiers,
                        );
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// SGR reports carry 1-based terminal-cell coordinates; Crossterm events
    /// are 0-based. Wheel, primary press, drag, and release are normalized
    /// here; unsupported buttons are dropped.
    fn consume_sgr_mouse(&mut self, params: &str, final_byte: u8) -> bool {
        let Some(rest) = params.strip_prefix('<') else {
            return false;
        };
        let mut parts = rest.split(';');
        let button = parts.next();
        let Some(column) = parts
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .and_then(|value| value.checked_sub(1))
        else {
            return false;
        };
        let Some(row) = parts
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .and_then(|value| value.checked_sub(1))
        else {
            return false;
        };
        let button = button.and_then(|value| value.parse::<u16>().ok());
        let Some(button) = button else { return false };
        let mut modifiers = KeyModifiers::NONE;
        if button & 4 != 0 {
            modifiers |= KeyModifiers::SHIFT;
        }
        if button & 8 != 0 {
            modifiers |= KeyModifiers::ALT;
        }
        if button & 16 != 0 {
            modifiers |= KeyModifiers::CONTROL;
        }
        let code = button & !(4 | 8 | 16);
        let kind = if code == 64 {
            MouseEventKind::ScrollUp
        } else if code == 65 {
            MouseEventKind::ScrollDown
        } else if final_byte == b'm' && matches!(code & 3, 0 | 3) {
            MouseEventKind::Up(crossterm::event::MouseButton::Left)
        } else if code & 32 != 0 && code & 3 == 0 {
            MouseEventKind::Drag(crossterm::event::MouseButton::Left)
        } else if code & 3 == 0 {
            MouseEventKind::Down(crossterm::event::MouseButton::Left)
        } else {
            return false;
        };
        self.out.push_back(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }));
        true
    }

    /// Kitty CSI-u for the keys the client binds; other codepoints are dropped
    /// (the client never enables the kitty protocol, so these only appear from
    /// terminals that send CSI-u unconditionally).
    fn consume_csi_u(&mut self, params: &str) -> bool {
        let main = params.split(':').next().unwrap_or_default();
        let mut parts = main.split(';');
        let Some(codepoint) = parts.next().and_then(|v| v.parse::<u32>().ok()) else {
            return false;
        };
        let modifiers = parts
            .next()
            .map(|v| v.parse::<u8>().ok())
            .flatten()
            .and_then(|v| csi_modifiers(v))
            .unwrap_or(KeyModifiers::NONE);
        let code = match codepoint {
            27 => KeyCode::Esc,
            13 => KeyCode::Enter,
            9 => KeyCode::Tab,
            32 => KeyCode::Char(' '),
            127 => KeyCode::Backspace,
            _ => return false,
        };
        self.push_key(code, modifiers);
        true
    }

    fn try_ss3(&mut self) -> EscapeOutcome {
        if self.buf.len() < 3 {
            return EscapeOutcome::Pending;
        }
        let code = self.buf[2];
        self.buf.drain(..3);
        let key = match code {
            b'A' => Some(KeyCode::Up),
            b'B' => Some(KeyCode::Down),
            b'C' => Some(KeyCode::Right),
            b'D' => Some(KeyCode::Left),
            b'H' => Some(KeyCode::Home),
            b'F' => Some(KeyCode::End),
            b'M' => Some(KeyCode::Enter),
            _ => None,
        };
        if let Some(code) = key {
            self.out
                .push_back(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
            EscapeOutcome::Emitted
        } else {
            EscapeOutcome::Dropped
        }
    }

    fn emit_alt(&mut self, byte: u8) {
        let event = match byte {
            0x0d | 0x0a => KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            0x09 => KeyEvent::new(KeyCode::Tab, KeyModifiers::ALT),
            0x1b => KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT),
            0x7f | 0x08 => KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
            0x01..=0x1a => KeyEvent::new(
                KeyCode::Char((byte + 0x60) as char),
                KeyModifiers::ALT | KeyModifiers::CONTROL,
            ),
            0x20..=0x7e => KeyEvent::new(KeyCode::Char(byte as char), KeyModifiers::ALT),
            _ => return,
        };
        self.out.push_back(Event::Key(event));
    }

    fn push_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.out
            .push_back(Event::Key(KeyEvent::new(code, modifiers)));
    }

    /// CSI modifier parameter: value-1 is a bitmask (1 shift, 2 alt, 4 ctrl).
    fn modifiers_from_csi(&self, params: &str) -> KeyModifiers {
        let mut parts = params.split(';');
        // Skip the function-identifying first parameter (e.g. "1" in "1;5").
        let _ = parts.next();
        parts
            .next()
            .and_then(|v| v.parse::<u8>().ok())
            .and_then(csi_modifiers)
            .unwrap_or(KeyModifiers::NONE)
    }
}

fn csi_modifiers(value: u8) -> Option<KeyModifiers> {
    let bits = value.checked_sub(1)?;
    let mut modifiers = KeyModifiers::NONE;
    if bits & 1 != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if bits & 2 != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    if bits & 4 != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    Some(modifiers)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Windows clipboards and terminals deliver `\r\n` (or lone `\r`) line
/// endings; the composer's internal newline is `\n`.
fn normalize_paste(bytes: &[u8]) -> String {
    crate::input::normalize_paste_text(&String::from_utf8_lossy(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{Event, KeyEventKind};

    fn parser() -> VtInputParser {
        VtInputParser::new(Box::new(|| NativeMods::default()))
    }

    fn mods_parser(mods: NativeMods) -> VtInputParser {
        VtInputParser::new(Box::new(move || mods))
    }

    fn feed_all(parser: &mut VtInputParser, bytes: &[u8]) -> Vec<Event> {
        parser.feed(bytes);
        let mut events = Vec::new();
        while let Some(event) = parser.pop() {
            events.push(event);
        }
        events
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_text_and_cjk_decode_to_chars() {
        let mut parser = parser();
        let events = feed_all(&mut parser, "hello 世界".as_bytes());
        assert_eq!(
            events,
            vec![
                key(KeyCode::Char('h'), KeyModifiers::NONE),
                key(KeyCode::Char('e'), KeyModifiers::NONE),
                key(KeyCode::Char('l'), KeyModifiers::NONE),
                key(KeyCode::Char('l'), KeyModifiers::NONE),
                key(KeyCode::Char('o'), KeyModifiers::NONE),
                key(KeyCode::Char(' '), KeyModifiers::NONE),
                key(KeyCode::Char('世'), KeyModifiers::NONE),
                key(KeyCode::Char('界'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn utf8_split_across_chunks_is_reassembled() {
        let mut parser = parser();
        let bytes = "中".as_bytes(); // 3 bytes
        parser.feed(&bytes[..2]);
        assert!(parser.pop().is_none(), "incomplete UTF-8 is held");
        assert_eq!(parser.pending(), Pending::None);
        parser.feed(&bytes[2..]);
        assert_eq!(
            parser.pop(),
            Some(key(KeyCode::Char('中'), KeyModifiers::NONE))
        );
    }

    #[test]
    fn control_bytes_map_to_keys() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"\x03\x0e\x10\x19\x0c\x12\t\x7f");
        assert_eq!(
            events,
            vec![
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                key(KeyCode::Char('n'), KeyModifiers::CONTROL),
                key(KeyCode::Char('p'), KeyModifiers::CONTROL),
                key(KeyCode::Char('y'), KeyModifiers::CONTROL),
                key(KeyCode::Char('l'), KeyModifiers::CONTROL),
                key(KeyCode::Char('r'), KeyModifiers::CONTROL),
                key(KeyCode::Tab, KeyModifiers::NONE),
                key(KeyCode::Backspace, KeyModifiers::NONE),
            ]
        );
    }

    /// The observed Windows Terminal byte table, captured with
    /// `cargo run -p e-dsh --example input_probe`. These four rows are the
    /// ground truth this parser must reproduce.
    #[test]
    fn windows_terminal_backspace_byte_table() {
        // Backspace -> DEL, no physical Ctrl.
        let mut plain = parser();
        plain.feed_with_native_mods(
            b"\x7f",
            NativeMods {
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            plain.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::NONE))
        );

        // Ctrl+Backspace -> ETB, with Ctrl and Backspace both held.
        let mut ctrl_backspace = parser();
        ctrl_backspace.feed_with_native_mods(
            b"\x17",
            NativeMods {
                ctrl: true,
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            ctrl_backspace.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::CONTROL))
        );

        // Ctrl+H -> BS, with Ctrl held but no physical Backspace. The help
        // binding depends on this staying Ctrl+H.
        let mut ctrl_h = parser();
        ctrl_h.feed_with_native_mods(
            b"\x08",
            NativeMods {
                ctrl: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            ctrl_h.pop(),
            Some(key(KeyCode::Char('h'), KeyModifiers::CONTROL))
        );

        // Alt+Backspace -> ESC DEL.
        let mut alt = parser();
        alt.feed_with_native_mods(
            b"\x1b\x7f",
            NativeMods {
                alt: true,
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(alt.pop(), Some(key(KeyCode::Backspace, KeyModifiers::ALT)));
    }

    /// ETB without a physical Backspace is a genuine Ctrl+W, which the
    /// composer also treats as delete-word.
    #[test]
    fn etb_without_backspace_snapshot_stays_ctrl_w() {
        let mut parser = parser();
        parser.feed_with_native_mods(
            b"\x17",
            NativeMods {
                ctrl: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            parser.pop(),
            Some(key(KeyCode::Char('w'), KeyModifiers::CONTROL))
        );
    }

    /// A coalesced chunk must not let a later Ctrl+Backspace promote an earlier
    /// plain Backspace: only the final byte may use the physical Backspace
    /// snapshot (sampled after the read returned).
    #[test]
    fn coalesced_chunk_does_not_promote_earlier_backspace() {
        let mut parser = parser();
        parser.feed_with_native_mods(
            b"\x7f\x17",
            NativeMods {
                ctrl: true,
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            parser.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::NONE))
        );
        assert_eq!(
            parser.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn raw_bs_without_ctrl_is_plain_backspace() {
        let mut legacy = parser();
        assert_eq!(
            feed_all(&mut legacy, b"\x08"),
            vec![key(KeyCode::Backspace, KeyModifiers::NONE)]
        );
    }

    #[test]
    fn raw_del_ignores_ctrl_without_a_backspace_snapshot() {
        // Ctrl alone must never transform DEL into Ctrl+Backspace.
        let mut parser = mods_parser(NativeMods {
            ctrl: true,
            ..NativeMods::default()
        });
        assert_eq!(
            feed_all(&mut parser, b"\x7f"),
            vec![key(KeyCode::Backspace, KeyModifiers::NONE)]
        );
    }

    #[test]
    fn reader_time_snapshot_disambiguates_raw_backspace_and_ctrl_h() {
        let mut del = parser();
        del.feed_with_native_mods(
            b"\x7f",
            NativeMods {
                ctrl: true,
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            del.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::CONTROL))
        );

        let mut bs = parser();
        bs.feed_with_native_mods(
            b"\x08",
            NativeMods {
                ctrl: true,
                back: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            bs.pop(),
            Some(key(KeyCode::Backspace, KeyModifiers::CONTROL))
        );

        let mut ctrl_h = parser();
        ctrl_h.feed_with_native_mods(
            b"\x08",
            NativeMods {
                ctrl: true,
                ..NativeMods::default()
            },
        );
        assert_eq!(
            ctrl_h.pop(),
            Some(key(KeyCode::Char('h'), KeyModifiers::CONTROL))
        );
    }

    #[test]
    fn escape_then_del_is_alt_backspace() {
        // macOS Option+Backspace arrives as ESC DEL on the Unix path; the
        // Windows raw stream can carry the same encoding.
        let mut parser = parser();
        assert_eq!(
            feed_all(&mut parser, b"\x1b\x7f"),
            vec![key(KeyCode::Backspace, KeyModifiers::ALT)]
        );
    }

    #[test]
    fn cr_and_lf_are_enter() {
        let mut parser = parser();
        assert_eq!(
            feed_all(&mut parser, b"\r\n"),
            vec![
                key(KeyCode::Enter, KeyModifiers::NONE),
                key(KeyCode::Enter, KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn enter_carries_native_modifiers() {
        let mut shift = mods_parser(NativeMods {
            shift: true,
            ..NativeMods::default()
        });
        assert_eq!(
            feed_all(&mut shift, b"\r"),
            vec![key(KeyCode::Enter, KeyModifiers::SHIFT)]
        );
        let mut ctrl = mods_parser(NativeMods {
            ctrl: true,
            ..NativeMods::default()
        });
        assert_eq!(
            feed_all(&mut ctrl, b"\r"),
            vec![key(KeyCode::Enter, KeyModifiers::CONTROL)]
        );
        let mut alt = mods_parser(NativeMods {
            alt: true,
            ..NativeMods::default()
        });
        assert_eq!(
            feed_all(&mut alt, b"\r"),
            vec![key(KeyCode::Enter, KeyModifiers::ALT)]
        );
    }

    #[test]
    fn lone_escape_flushes_after_timeout() {
        let mut parser = parser();
        parser.feed(b"\x1b");
        assert!(parser.pop().is_none());
        assert_eq!(parser.pending(), Pending::Escape);
        parser.flush_timeout();
        assert_eq!(parser.pop(), Some(key(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn escape_then_char_is_alt_key() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"\x1bq\x1b\r\x1b\x1b");
        assert_eq!(
            events,
            vec![
                key(KeyCode::Char('q'), KeyModifiers::ALT),
                key(KeyCode::Enter, KeyModifiers::ALT),
                key(KeyCode::Esc, KeyModifiers::ALT),
            ]
        );
    }

    #[test]
    fn navigation_sequences() {
        let mut parser = parser();
        let events = feed_all(
            &mut parser,
            b"\x1b[A\x1b[B\x1b[C\x1b[D\x1b[H\x1b[F\x1b[3~\x1b[5~\x1b[6~\x1b[Z",
        );
        assert_eq!(
            events,
            vec![
                key(KeyCode::Up, KeyModifiers::NONE),
                key(KeyCode::Down, KeyModifiers::NONE),
                key(KeyCode::Right, KeyModifiers::NONE),
                key(KeyCode::Left, KeyModifiers::NONE),
                key(KeyCode::Home, KeyModifiers::NONE),
                key(KeyCode::End, KeyModifiers::NONE),
                key(KeyCode::Delete, KeyModifiers::NONE),
                key(KeyCode::PageUp, KeyModifiers::NONE),
                key(KeyCode::PageDown, KeyModifiers::NONE),
                key(KeyCode::Tab, KeyModifiers::SHIFT),
            ]
        );
    }

    #[test]
    fn ss3_application_arrows() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"\x1bOA\x1bOB\x1bOC\x1bOD\x1bOM");
        assert_eq!(
            events,
            vec![
                key(KeyCode::Up, KeyModifiers::NONE),
                key(KeyCode::Down, KeyModifiers::NONE),
                key(KeyCode::Right, KeyModifiers::NONE),
                key(KeyCode::Left, KeyModifiers::NONE),
                key(KeyCode::Enter, KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn modified_navigation_and_tilde_keys() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"\x1b[1;5A\x1b[1;3D\x1b[3;5~\x1b[1;2B");
        assert_eq!(
            events,
            vec![
                key(KeyCode::Up, KeyModifiers::CONTROL),
                key(KeyCode::Left, KeyModifiers::ALT),
                key(KeyCode::Delete, KeyModifiers::CONTROL),
                key(KeyCode::Down, KeyModifiers::SHIFT),
            ]
        );
    }

    #[test]
    fn modify_other_keys_forms_include_escape() {
        let mut parser = parser();
        let events = feed_all(
            &mut parser,
            b"\x1b[27;1;27~\x1b[27;2;13~\x1b[27;3;13~\x1b[27;5;13~\x1b[27;2;9~\x1b[27;5;97~\x1b[27;5;127~",
        );
        assert_eq!(
            events,
            vec![
                key(KeyCode::Esc, KeyModifiers::NONE),
                key(KeyCode::Enter, KeyModifiers::SHIFT),
                key(KeyCode::Enter, KeyModifiers::ALT),
                key(KeyCode::Enter, KeyModifiers::CONTROL),
                key(KeyCode::Tab, KeyModifiers::SHIFT),
                key(KeyCode::Char('a'), KeyModifiers::CONTROL),
                key(KeyCode::Backspace, KeyModifiers::CONTROL),
            ]
        );
    }

    #[test]
    fn csi_u_forms_include_escape() {
        let mut parser = parser();
        let events = feed_all(
            &mut parser,
            b"\x1b[27u\x1b[13;2u\x1b[13;3u\x1b[13;5u\x1b[9;5u\x1b[127;5u",
        );
        assert_eq!(
            events,
            vec![
                key(KeyCode::Esc, KeyModifiers::NONE),
                key(KeyCode::Enter, KeyModifiers::SHIFT),
                key(KeyCode::Enter, KeyModifiers::ALT),
                key(KeyCode::Enter, KeyModifiers::CONTROL),
                key(KeyCode::Tab, KeyModifiers::CONTROL),
                key(KeyCode::Backspace, KeyModifiers::CONTROL),
            ]
        );
    }

    #[test]
    fn bracketed_paste_becomes_one_event_with_normalized_newlines() {
        let mut parser = parser();
        let payload = "line1\r\nline2\rline3\tend";
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend_from_slice(payload.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        let events = feed_all(&mut parser, &bytes);
        assert_eq!(
            events,
            vec![Event::Paste("line1\nline2\nline3\tend".into())]
        );
    }

    #[test]
    fn paste_marker_split_across_chunks() {
        let mut parser = parser();
        parser.feed(b"\x1b[20");
        assert!(parser.pop().is_none(), "partial marker is held");
        assert_eq!(parser.pending(), Pending::Sequence);
        parser.feed(b"0~abc\x1b[20");
        assert!(parser.pop().is_none(), "still inside the paste");
        parser.feed(b"1~tail");
        let mut events = Vec::new();
        while let Some(event) = parser.pop() {
            events.push(event);
        }
        assert_eq!(
            events,
            vec![
                Event::Paste("abc".into()),
                key(KeyCode::Char('t'), KeyModifiers::NONE),
                key(KeyCode::Char('a'), KeyModifiers::NONE),
                key(KeyCode::Char('i'), KeyModifiers::NONE),
                key(KeyCode::Char('l'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn paste_without_close_marker_is_held_until_timeout_flush() {
        let mut parser = parser();
        parser.feed(b"\x1b[200~abc");
        assert!(parser.pop().is_none());
        // A stuck paste cannot be rescued by the timeout flush; the reader
        // only drops incomplete sequences, and paste mode waits for the
        // closing marker. This mirrors the terminal protocol: a paste that
        // never closes leaves the stream in paste mode.
        parser.flush_timeout();
        assert!(parser.pop().is_none());
        parser.feed(b"\x1b[201~");
        assert_eq!(parser.pop(), Some(Event::Paste("abc".into())));
    }

    #[test]
    fn incomplete_sequence_is_dropped_on_timeout() {
        let mut parser = parser();
        parser.feed(b"\x1b[1;");
        assert!(parser.pop().is_none());
        assert_eq!(parser.pending(), Pending::Sequence);
        parser.flush_timeout();
        assert!(parser.pop().is_none(), "truncated sequence is dropped");
    }

    #[test]
    fn unknown_csi_is_dropped_immediately() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"\x1b[25~x");
        assert_eq!(
            events,
            vec![key(KeyCode::Char('x'), KeyModifiers::NONE)],
            "unbound function-key sequences are ignored"
        );
    }

    #[test]
    fn sgr_mouse_scroll_and_primary_drag_are_parsed_across_chunks() {
        let mut parser = parser();
        parser.feed(b"\x1b[<68;5;10M\x1b[<0;5");
        parser.feed(b";10M\x1b[<32;6;10M\x1b[<0;6;10m\x1b[<81;3;7M\x1b[<2;1;1M\x1b[<0;0;1M");
        let events = std::iter::from_fn(|| parser.pop()).collect::<Vec<_>>();
        assert_eq!(
            events,
            vec![
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 4,
                    row: 9,
                    modifiers: KeyModifiers::SHIFT,
                }),
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
                    column: 4,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(crossterm::event::MouseButton::Left),
                    column: 5,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(crossterm::event::MouseButton::Left),
                    column: 5,
                    row: 9,
                    modifiers: KeyModifiers::NONE,
                }),
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 2,
                    row: 6,
                    modifiers: KeyModifiers::CONTROL,
                }),
            ]
        );
    }

    #[test]
    fn focus_reports_are_parsed_without_reaching_the_composer() {
        let mut parser = parser();
        parser.feed(b"\x1b[");
        parser.feed(b"I\x1b[O");
        assert_eq!(parser.pop(), Some(Event::FocusGained));
        assert_eq!(parser.pop(), Some(Event::FocusLost));
        assert_eq!(parser.pop(), None);
    }

    #[test]
    fn key_events_are_press_only() {
        let mut parser = parser();
        let events = feed_all(&mut parser, b"a");
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::Key(key) => assert_eq!(key.kind, KeyEventKind::Press),
            _ => panic!("expected key event"),
        }
    }
}
