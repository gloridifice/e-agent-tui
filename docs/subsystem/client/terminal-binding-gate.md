# Configurable terminal binding gate

> Status: Current
> Authority: Keyboard compatibility validation procedure. Defaults live in the root mapping TOML, not in this checklist.

The historical [Ctrl+Y investigation](../../history/rust-client-refactor/terminal-binding-gate.md) remains frozen. Current defaults use osmain-R for Reading and osmain-V for application paste; no new physical-terminal measurements are inferred from the historical report.

## Automated gate

```powershell
cargo test -p e-tui --lib key_mapping
cargo test -p e-tui --lib runtime::input
cargo test -p e-tui --lib input
cargo test -p e-tui --lib runtime::controller
cargo test -p e-dsh --test architecture
```

The gate checks exact platform-aware chords, removed defaults, disabled actions, protected pages and approvals, working queue order, atomic reload, Reading exit/Item return and complete-source copy. Raw VT tests preserve Windows BS/DEL/ETB handling, modified Enter, Kitty CSI-u, modifyOtherKeys, partial-Escape deadlines, bracketed paste, and Ctrl+V fallback delivery. These tests establish parser/router behavior, not physical shortcut delivery by every terminal.

## Manual matrix

Before claiming physical compatibility for a terminal family, test Windows Terminal and ConHost, macOS terminal applications, and xterm/VTE/kitty-compatible Linux terminals independently. For each terminal, record version, keyboard protocol, configured host shortcuts, and observed events using the adapter's `input_probe` example.

1. Exercise help, Reading entry, model, effort, settings, resume and Preview defaults. On macOS verify whether Command combinations reach the application at all.
2. Verify plain/Shift/osmain Enter are distinguishable and produce their configured actions, without sending on pasted line breaks.
3. Rebind a shortcut to F1, reload, and verify the old binding stops invoking that action and all subsequent hints use F1.
4. Disable application paste; verify a delivered Ctrl+V key/fallback no longer reads the clipboard, while independent bracketed text paste remains text.
5. Enter Reading with a multiline/image draft, navigate Blocks and Items, and verify PageDown/PageUp matches 15 ordinary down/up cursor steps (not viewport-only scrolling), including Item-to-Block transitions and document boundaries. Disable or remap the fast actions and verify there is no global paging fallback. Copy complete source, use Backspace to return to Blocks, and Esc/q to restore the draft.
6. Open a text editor, question or approval and verify global picker shortcuts do not replace it; verify ordinary hjklq text and explicit approval responses.

Physical runs for the new defaults must be recorded separately; automated success alone does not mark this matrix complete.
