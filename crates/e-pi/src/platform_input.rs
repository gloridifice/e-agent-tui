//! Pi executable's narrow Windows console shim for shared terminal input.

#[cfg(windows)]
pub fn native_mods() -> e_tui::runtime::input::vt::NativeMods {
    use winapi::um::winuser::{GetAsyncKeyState, VK_BACK, VK_CONTROL, VK_MENU, VK_SHIFT};

    fn down(key: i32) -> bool {
        // SAFETY: `GetAsyncKeyState` accepts any virtual-key code and has no
        // pointer or lifetime preconditions.
        unsafe { (GetAsyncKeyState(key) as u16) & 0x8000 != 0 }
    }

    e_tui::runtime::input::vt::NativeMods {
        shift: down(VK_SHIFT),
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        back: down(VK_BACK),
    }
}

#[cfg(windows)]
pub fn enable_virtual_terminal_input() -> std::io::Result<()> {
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::STD_INPUT_HANDLE;
    use winapi::um::wincon::ENABLE_VIRTUAL_TERMINAL_INPUT;

    // SAFETY: the standard-input handle is validated before it is passed to
    // console mode APIs, and the mode pointer refers to a live local `u32`.
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut mode = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
