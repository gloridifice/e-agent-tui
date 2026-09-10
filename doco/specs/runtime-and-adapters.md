# Runtime and adapter contracts

## Dependency boundary

- `e-tui` MUST remain provider-neutral and MUST NOT import DSH/Pi wire names, transport control, provider persistence, or filesystem-backed effects.
- `e-dsh` and `e-pi` MUST depend on `e-tui` and MUST NOT depend on each other.
- Each adapter MUST convert provider input into owned `AgentEvent` values and convert `AgentRequest` values at its boundary. Raw provider JSON MUST NOT enter the frontend reducer.
- Architecture tests MUST reject production module cycles and forbidden cross-package dependencies.

## Runtime effects and locking

- `RuntimeController` MUST return owned actions/effects. Runners MUST release frontend state guards before awaiting or performing external work.
- The shared executor MUST process effects in order and return every completion for reduction; it MUST NOT borrow UI state or silently drop effects.
- Code MUST NOT re-lock or await while an `if let` or `match` scrutinee still owns a state guard.
- Terminal setup, restoration, event routing, synchronized frame submission, and shared scheduling policy belong to `e-tui`; provider selection loops and external ports remain adapter-owned.

## Scheduling and transport bounds

- Idle runtime waits MUST remain deadline-driven; a fixed polling ticker is forbidden.
- Animation cadence MUST be at least 16 ms. Each inbound fairness turn MUST stop at 64 items or 2 ms, and visible assistant deltas MUST yield to rendering.
- DSH frames and Pi JSONL records MUST be bounded before untrusted payloads reach reducers. Pi JSONL accepts LF records, tolerates CRLF, fails closed on invalid UTF-8/JSON or oversized records, and rejects an unterminated final record.
- Terminal frames MUST be submitted atomically. A selectable screen snapshot becomes current only after successful submission and MUST NOT enter semantic caches.

## Platform input

- Windows raw VT parsing MUST preserve bracketed paste, modified navigation, mouse input, Escape timeouts, and Backspace/Ctrl+Backspace distinctions.
- Clipboard access remains adapter-owned. Shared input receives normalized text or provider-neutral image data; paste and image blocks MUST preserve prompt-part order.
