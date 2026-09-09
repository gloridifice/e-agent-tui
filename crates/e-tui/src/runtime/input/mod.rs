//! Terminal event normalization and provider-independent VT parsing.

pub mod source;
pub mod vt;

pub use source::ProductionTerminalEvents;

use crossterm::event::{Event, KeyEvent, KeyEventKind, MouseButton, MouseEventKind};

use crate::key_mapping::{Action, KeyMapping, Scope};
use crate::PointerEvent;
#[cfg(test)]
use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalFocus {
    pub help_visible: bool,
    pub input_page_open: bool,
    pub approval_open: bool,
    pub reading_view_open: bool,
    pub history_view_open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalRoute {
    Pointer(PointerEvent),
    Paste { text: String },
    ReadClipboard,
    Help { dismiss: bool },
    OpenHelp,
    Global(Action),
    TranscriptPage { up: bool },
    InputPage(KeyEvent),
    Approval(KeyEvent),
    Reading(KeyEvent),
    History(KeyEvent),
    Ordinary(KeyEvent),
    Ignore,
}

#[cfg(test)]
fn route_terminal_event(event: Event, focus: TerminalFocus) -> TerminalRoute {
    route_terminal_event_with_mapping(event, focus, &KeyMapping::default())
}

pub fn route_terminal_event_with_mapping(
    event: Event,
    focus: TerminalFocus,
    mapping: &KeyMapping,
) -> TerminalRoute {
    let scope = if focus.reading_view_open && !focus.input_page_open && !focus.approval_open {
        Scope::ReadMode
    } else {
        Scope::Global
    };
    let global = match &event {
        Event::Key(key) => mapping.resolve_global(scope, key),
        _ => None,
    };
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                TerminalRoute::Pointer(PointerEvent::Wheel {
                    up: mouse.kind == MouseEventKind::ScrollUp,
                    column: mouse.column,
                    row: mouse.row,
                })
            }
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
        Event::Paste(_) if focus.reading_view_open || focus.history_view_open => {
            TerminalRoute::Ignore
        }
        // A terminal that consumed the paste shortcut but had no text (Windows
        // Terminal with an image-only clipboard) delivers an EMPTY bracketed
        // paste instead of nothing at all. Treat that as the terminal handing
        // the paste back: read the clipboard through the application port,
        // which prefers image content over text.
        Event::Paste(text)
            if text.is_empty()
                && !focus.input_page_open
                && !focus.approval_open
                && !focus.help_visible =>
        {
            TerminalRoute::ReadClipboard
        }
        Event::Paste(_) if focus.approval_open || focus.help_visible => TerminalRoute::Ignore,
        Event::Paste(text) => TerminalRoute::Paste { text },
        Event::Key(key) if key.kind == KeyEventKind::Release => TerminalRoute::Ignore,
        Event::Key(key) if focus.history_view_open => TerminalRoute::History(key),
        Event::Key(key) if focus.help_visible => TerminalRoute::Help {
            dismiss: mapping.resolve(Scope::Help, &key) == Some(Action::Close)
                || mapping.resolve(Scope::Global, &key) == Some(Action::PrintHelp),
        },
        Event::Key(_) if global.is_some() => match global.unwrap() {
            Action::PrintHelp => TerminalRoute::OpenHelp,
            Action::PageUp => TerminalRoute::TranscriptPage { up: true },
            Action::PageDown => TerminalRoute::TranscriptPage { up: false },
            _ if focus.input_page_open || focus.approval_open || focus.reading_view_open => {
                TerminalRoute::Ignore
            }
            action => TerminalRoute::Global(action),
        },
        Event::Key(key) if focus.input_page_open => TerminalRoute::InputPage(key),
        Event::Key(key) if focus.approval_open => TerminalRoute::Approval(key),
        Event::Key(key)
            if focus.reading_view_open
                && mapping.resolve(Scope::Message, &key) == Some(Action::Paste)
                && mapping.resolve(Scope::ReadMode, &key).is_none()
                && mapping.resolve(Scope::ReadModeItem, &key).is_none() =>
        {
            TerminalRoute::Ignore
        }
        Event::Key(key) if focus.reading_view_open => TerminalRoute::Reading(key),
        Event::Key(key) if mapping.resolve(Scope::Message, &key) == Some(Action::Paste) => {
            TerminalRoute::ReadClipboard
        }
        Event::Key(key) => TerminalRoute::Ordinary(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_preserves_coordinates_in_every_focus() {
        for kind in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown] {
            for focus in [
                TerminalFocus::default(),
                TerminalFocus {
                    input_page_open: true,
                    ..Default::default()
                },
                TerminalFocus {
                    history_view_open: true,
                    ..Default::default()
                },
                TerminalFocus {
                    reading_view_open: true,
                    ..Default::default()
                },
            ] {
                assert_eq!(
                    route_terminal_event(
                        Event::Mouse(crossterm::event::MouseEvent {
                            kind,
                            column: 91,
                            row: 17,
                            modifiers: KeyModifiers::NONE,
                        }),
                        focus
                    ),
                    TerminalRoute::Pointer(PointerEvent::Wheel {
                        up: kind == MouseEventKind::ScrollUp,
                        column: 91,
                        row: 17,
                    }),
                );
            }
        }
    }

    #[test]
    fn quick_links_entry_respects_protected_focus_and_exact_modifiers() {
        let entry = Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(
            route_terminal_event(entry.clone(), TerminalFocus::default()),
            TerminalRoute::Global(Action::CopyLink)
        );
        for focus in [
            TerminalFocus {
                input_page_open: true,
                ..Default::default()
            },
            TerminalFocus {
                approval_open: true,
                ..Default::default()
            },
            TerminalFocus {
                reading_view_open: true,
                ..Default::default()
            },
            TerminalFocus {
                history_view_open: true,
                ..Default::default()
            },
            TerminalFocus {
                help_visible: true,
                ..Default::default()
            },
        ] {
            assert_ne!(
                route_terminal_event(entry.clone(), focus),
                TerminalRoute::Global(Action::CopyLink)
            );
        }
        assert_ne!(
            route_terminal_event(
                Event::Key(KeyEvent::new(
                    KeyCode::Char('y'),
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT
                )),
                TerminalFocus::default()
            ),
            TerminalRoute::Global(Action::CopyLink)
        );
    }

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
        // An image-only clipboard makes some terminals deliver an empty
        // bracketed paste; Reading View must suppress that path as well.
        assert_eq!(
            route_terminal_event(Event::Paste(String::new()), focus),
            TerminalRoute::Ignore
        );
    }

    #[test]
    fn empty_bracketed_paste_reads_the_clipboard_itself() {
        // Windows Terminal consumes Ctrl+V and, with an image-only clipboard,
        // delivers `ESC[200~ESC[201~` with no text. That empty paste is the
        // terminal handing the shortcut back: route it to the application
        // clipboard read so image content can still be pasted.
        assert_eq!(
            route_terminal_event(Event::Paste(String::new()), TerminalFocus::default()),
            TerminalRoute::ReadClipboard
        );
        assert_eq!(
            route_terminal_event(Event::Paste("content".into()), TerminalFocus::default()),
            TerminalRoute::Paste {
                text: "content".into()
            }
        );
        // Whitespace-only paste text is still real text and pastes verbatim.
        assert_eq!(
            route_terminal_event(Event::Paste(" \n ".into()), TerminalFocus::default()),
            TerminalRoute::Paste {
                text: " \n ".into()
            }
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
