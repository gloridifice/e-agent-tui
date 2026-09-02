//! Paste diagnostic probe: runs the exact production terminal event source
//! (`ProductionTerminalEvents`, including the physical Ctrl+V fallback) and
//! appends every returned event to a log file so an external driver can see
//! whether the synthetic shortcut fires and what the terminal delivers.
//!
//! Run: `cargo run -p e-dsh --example paste_diag -- <log-path>`
//! Press Ctrl+C to quit.

use std::{io::Write, time::SystemTime};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use e_tui::runtime::{ProductionTerminalEvents, TerminalEventPort};

fn stamp() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[tokio::main]
async fn main() {
    let log_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "paste_diag.log".to_owned());
    let mut log = std::fs::File::create(&log_path).expect("create diag log");
    let mut stdout = std::io::stdout();

    crossterm::terminal::enable_raw_mode().expect("enable raw mode");
    // Must run after raw mode, whose setup otherwise clears the flag.
    let setup = e::win_input::enable_virtual_terminal_input();

    let _ = writeln!(log, "READY t={} setup={:?}", stamp(), setup);
    let _ = log.flush();
    let _ = writeln!(stdout, "paste diag ready; log: {log_path}\r");
    let _ = stdout.flush();

    #[cfg(windows)]
    let mut events = ProductionTerminalEvents::new(e::win_input::native_mods);
    #[cfg(not(windows))]
    let mut events = ProductionTerminalEvents::new();

    while let Some(event) = events.next_event().await {
        let line = match &event {
            Ok(inner) => format!("t={} EVENT {:?}", stamp(), inner),
            Err(error) => format!("t={} ERROR {error}", stamp()),
        };
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
        let _ = writeln!(stdout, "{line}\r");
        let _ = stdout.flush();
        let quit = matches!(
            &event,
            Ok(Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            })) if modifiers.contains(KeyModifiers::CONTROL)
        );
        if quit {
            break;
        }
    }

    let _ = writeln!(log, "EOF t={}", stamp());
    let _ = crossterm::terminal::disable_raw_mode();
}
