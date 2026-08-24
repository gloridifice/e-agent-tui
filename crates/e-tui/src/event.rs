//! Terminal input values accepted by the frontend state machine.

use crossterm::event::KeyEvent;

/// Frontend pointer gestures after terminal-specific decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEvent {
    Wheel { up: bool },
    PrimaryPress { column: u16, row: u16 },
    PrimaryDrag { column: u16, row: u16 },
    PrimaryRelease { column: u16, row: u16 },
    FocusLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Pointer(PointerEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
}
