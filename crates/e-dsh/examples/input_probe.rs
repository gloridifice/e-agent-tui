//! Raw Windows terminal input probe.
//!
//! Reproduces the production input setup exactly (raw mode plus
//! `ENABLE_VIRTUAL_TERMINAL_INPUT`) and prints, for every stdin read, the raw
//! bytes, the reader-time physical key snapshot, and the parsed crossterm
//! event. Use it to find out what a specific terminal actually sends for keys
//! such as Ctrl+Backspace instead of guessing at an encoding.
//!
//! Run: `cargo run -p e-dsh --example input_probe`. Press Ctrl+C to quit.

fn main() -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        eprintln!("input_probe targets the Windows raw-VT input path.");
        Ok(())
    }

    #[cfg(windows)]
    {
        use std::io::Read;

        use e_tui::runtime::input::vt::VtInputParser;

        crossterm::terminal::enable_raw_mode()?;
        // Must run after raw mode, whose setup otherwise clears the flag.
        e_dsh::win_input::enable_virtual_terminal_input()?;

        println!("Input probe. Press keys; Ctrl+C quits.\r");
        println!("Try: Backspace, Ctrl+Backspace, Ctrl+H, Alt+Backspace.\r");
        println!("---\r");

        let mut parser = VtInputParser::new(Box::new(native_mods));
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let mut buf = [0u8; 8192];

        loop {
            let read = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let bytes = &buf[..read];
            let mods = native_mods();

            let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            println!(
                "bytes: [{}]  snapshot: ctrl={} back={} shift={} alt={}\r",
                hex.join(" "),
                mods.ctrl,
                mods.back,
                mods.shift,
                mods.alt,
            );

            parser.feed_with_native_mods(bytes, mods);
            while let Some(event) = parser.pop() {
                println!("  parsed: {event:?}\r");
            }

            if bytes.contains(&0x03) {
                break;
            }
        }

        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }
}

#[cfg(windows)]
fn native_mods() -> e_tui::runtime::input::vt::NativeMods {
    use winapi::um::winuser::{GetAsyncKeyState, VK_BACK, VK_CONTROL, VK_MENU, VK_SHIFT};

    const VK_V: i32 = b'V' as i32;

    fn down(key: i32) -> bool {
        unsafe { (GetAsyncKeyState(key) as u16) & 0x8000 != 0 }
    }

    e_tui::runtime::input::vt::NativeMods {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        paste: down(VK_CONTROL) && down(VK_V),
        back: down(VK_BACK),
    }
}
