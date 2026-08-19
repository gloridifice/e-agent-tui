# Reading View terminal binding gate

The original `Ctrl+V` candidate does not pass the supported-terminal gate. Windows Terminal reserves `Ctrl+V` for paste in its default configuration, and current ConHost configurations may also translate it into a paste operation before Crossterm can observe a key event. That makes it unsuitable as the only Reading View entry gesture even though bracketed paste itself continues to arrive as `Event::Paste`.

The selected binding is **`Ctrl+Y`**. It is unclaimed by dshe's global router, composer, Input Pages, approvals, and Preview toggle, and the supported terminal families deliver the corresponding control key without taking ownership of paste.

| Terminal family | `Ctrl+V` result | `Ctrl+Y` result | Bracketed paste |
| --- | --- | --- | --- |
| Windows Terminal | Reserved/default paste; candidate fails | Delivered as control key | Delivered as paste event |
| ConHost | Paste handling varies by host settings; candidate fails the universal gate | Delivered as control key | Delivered as paste event where supported |
| xterm/VTE-compatible Linux terminals | Delivery depends on terminal key configuration | Delivered as control key | Delivered as paste event |
| kitty-compatible Linux terminals | Configurable; not universal enough for the candidate | Delivered as control key | Delivered as paste event |

Automated routing tests assert that `Ctrl+Y` maps only to Reading View, higher-priority blocking interactions retain ownership, and paste events cannot mutate the composer while Reading View is active. The public help and key references use `Ctrl+Y` consistently.
