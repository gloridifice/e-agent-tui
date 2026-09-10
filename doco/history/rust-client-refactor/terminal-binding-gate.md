# Reading View terminal binding gate

> Status: Historical
> Authority: Non-normative. This compatibility investigation records why the implemented binding was selected.

The original `Ctrl+V` candidate does not pass the supported-terminal gate. Windows Terminal reserves `Ctrl+V` for paste in its default configuration, and current ConHost configurations may also translate it into a paste operation before Crossterm can observe a key event. That makes it unsuitable as the only Reading View entry gesture even though bracketed paste itself continues to arrive as `Event::Paste`.

The selected binding is **`Ctrl+Y`**. It is unclaimed by dshe's global router, composer, Input Pages, approvals, and Preview toggle, and the supported terminal families deliver the corresponding control key without taking ownership of paste.

| Terminal family | `Ctrl+V` result | `Ctrl+Y` result | Bracketed paste |
| --- | --- | --- | --- |
| Windows Terminal | Reserved/default paste; candidate fails | Delivered as control key | Delivered as paste event |
| ConHost | Paste handling varies by host settings; candidate fails the universal gate | Delivered as control key | Delivered as paste event where supported |
| xterm/VTE-compatible Linux terminals | Delivery depends on terminal key configuration | Delivered as control key | Delivered as paste event |
| kitty-compatible Linux terminals | Configurable; not universal enough for the candidate | Delivered as control key | Delivered as paste event |

Automated routing tests assert that `Ctrl+Y` maps only to Reading View, higher-priority blocking interactions retain ownership, and paste events cannot mutate the composer while Reading View is active. The public help and key references use `Ctrl+Y` consistently.

## Update (Windows paste delivery)

The "Delivered as paste event" rows above describe the *intended* delivery, but crossterm's Windows backend never
emits `Event::Paste` — its Windows event source only produces key/mouse/resize/focus events from console input
records, and the bracketed-paste parser (`ESC[200~ … ESC[201~`) exists only in the Unix backend
(`crossterm/src/event/sys/unix/parse.rs`). The console consumes the wrapper before records reach the application,
so pasted `\r` line endings arrive as plain Enter key events, which the composer treats as send. `dshe` therefore
replaces the Windows record source with raw VT byte input (pi's proven approach): `TerminalOwner` enables
`ENABLE_VIRTUAL_TERMINAL_INPUT`, a reader thread forwards stdin bytes, and `vt_input.rs` parses the byte stream —
including the bracketed-paste wrapper — into `Event::Paste`, with `win_input.rs` sampling the physical
Shift/Ctrl/Alt keys so `\r` Enter keeps its modifiers. This restores the paste event contract for Windows
Terminal and modern ConHost. Terminals that do not honor bracketed paste at all still deliver pastes as typed key
events and cannot be distinguished; that limitation is inherent to the terminal, not to the parser.
