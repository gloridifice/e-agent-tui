//! clipboard_check — verify the arboard system-clipboard integration on this
//! machine (copy mode writes through the same path).

fn main() {
    let test = "dsh-tui 剪贴板检查 ✓";
    let mut cb = arboard::Clipboard::new().expect("open clipboard");
    cb.set_text(test).expect("set clipboard");
    let back = cb.get_text().expect("get clipboard");
    assert_eq!(back, test, "roundtrip mismatch");
    println!("clipboard roundtrip OK: {back}");
}
