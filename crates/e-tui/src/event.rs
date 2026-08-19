//! Terminal input values accepted by the frontend state machine.

use crossterm::event::{KeyEvent, MouseEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
}
