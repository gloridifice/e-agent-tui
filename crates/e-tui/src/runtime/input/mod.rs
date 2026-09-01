//! Terminal event normalization and provider-independent VT parsing.

pub mod source;
pub mod vt;

pub use source::ProductionTerminalEvents;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use crate::PointerEvent;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalFocus {
    pub help_visible: bool,
    pub input_page_open: bool,
    pub approval_open: bool,
    pub reading_view_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRoute {
    Pointer(PointerEvent),
    Paste { text: String },
    ReadClipboard,
    Help { dismiss: bool },
    OpenHelp,
    TranscriptPage { up: bool },
    InputPage(KeyEvent),
    Approval(KeyEvent),
    Reading(KeyEvent),
    Ordinary(KeyEvent),
    Ignore,
}

pub fn route_terminal_event(event: Event, focus: TerminalFocus) -> TerminalRoute {
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => TerminalRoute::Pointer(PointerEvent::Wheel { up: true }),
            MouseEventKind::ScrollDown => TerminalRoute::Pointer(PointerEvent::Wheel { up: false }),
            MouseEventKind::Down(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryPress {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryDrag {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            MouseEventKind::Up(MouseButton::Left) => {
                TerminalRoute::Pointer(PointerEvent::PrimaryRelease {
                    column: mouse.column,
                    row: mouse.row,
                })
            }
            _ => TerminalRoute::Ignore,
        },
        Event::FocusLost | Event::Resize(_, _) => TerminalRoute::Pointer(PointerEvent::FocusLost),
        Event::FocusGained => TerminalRoute::Ignore,
        Event::Paste(_) if focus.reading_view_open => TerminalRoute::Ignore,
        Event::Paste(text) => TerminalRoute::Paste { text },
        Event::Key(key) if key.kind == KeyEventKind::Release => TerminalRoute::Ignore,
        Event::Key(key) if is_clipboard_paste_shortcut(&key) && focus.reading_view_open => {
            TerminalRoute::Ignore
        }
        Event::Key(key) if is_clipboard_paste_shortcut(&key) => TerminalRoute::ReadClipboard,
        Event::Key(key) if focus.help_visible => TerminalRoute::Help {
            dismiss: matches!(
                key.code,
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('h')
            ),
        },
        Event::Key(key)
            if key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            TerminalRoute::OpenHelp
        }
        Event::Key(key) if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) => {
            TerminalRoute::TranscriptPage {
                up: key.code == KeyCode::PageUp,
            }
        }
        Event::Key(key) if focus.input_page_open => TerminalRoute::InputPage(key),
        Event::Key(key) if focus.approval_open => TerminalRoute::Approval(key),
        Event::Key(key) if focus.reading_view_open => TerminalRoute::Reading(key),
        Event::Key(key) => TerminalRoute::Ordinary(key),
    }
}

fn is_clipboard_paste_shortcut(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('v' | 'V'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_view_suppresses_both_paste_paths() {
        let focus = TerminalFocus {
            reading_view_open: true,
            ..TerminalFocus::default()
        };
        assert_eq!(
            route_terminal_event(Event::Paste("text".into()), focus),
            TerminalRoute::Ignore
        );
        assert_eq!(
            route_terminal_event(
                Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
                focus,
            ),
            TerminalRoute::Ignore
        );
    }

    #[test]
    fn input_page_precedes_approval_and_reading() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let route = route_terminal_event(
            Event::Key(key),
            TerminalFocus {
                input_page_open: true,
                approval_open: true,
                reading_view_open: true,
                ..TerminalFocus::default()
            },
        );
        assert_eq!(route, TerminalRoute::InputPage(key));
    }
}
