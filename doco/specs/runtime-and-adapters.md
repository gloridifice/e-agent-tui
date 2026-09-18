# Runtime and adapter contracts

## Dependency boundary

- `e-tui` MUST remain provider-neutral and MUST NOT import DSH/Pi wire names, transport control, provider persistence, or filesystem-backed effects.
- `e-dsh` and `e-pi` MUST depend on `e-tui` and MUST NOT depend on each other.
- Each adapter MUST convert provider input into owned `AgentEvent` values and convert `AgentRequest` values at its boundary. Raw provider JSON MUST NOT enter the frontend reducer.
- Architecture tests MUST reject production module cycles and forbidden cross-package dependencies.

## Runtime effects and locking

- `RuntimeController` MUST return owned actions/effects. Runners MUST release frontend state guards before awaiting or performing external work.
- The shared executor MUST process effects in order and return every completion for reduction; it MUST NOT borrow UI state or silently drop effects.
- Quick-link filesystem validation MUST remain adapter-owned, bounded, and outside frontend state guards. URI targets require no probe. Workspace-relative targets MUST remain contained after lexical and symlink resolution. Absolute targets MUST exist in the native filesystem but MAY resolve outside the workspace; foreign-platform syntax and Windows UNC/device namespaces MUST be rejected without probing. Validation MUST NOT replace the source spelling returned for copy.
- `@` path completion MUST use the shared pure policy in `e-tui` for safe entry-name checks, case-insensitive contiguous-substring matching, directory-first ordering, and the bounded candidate result count. Adapters MUST retain directory reads, native path resolution, file-type/symlink probes, and blocking-task dispatch outside frontend state guards; unmatched names SHOULD be discarded before file-type probes.
- Code MUST NOT re-lock or await while an `if let` or `match` scrutinee still owns a state guard.
- Terminal setup, restoration, event routing, synchronized frame submission, and shared scheduling policy belong to `e-tui`; provider selection loops and external ports remain adapter-owned.
- Authentication requests/events are provider-neutral in `e-tui`. `e-pi` MUST execute Pi authentication through public native APIs outside frontend locks and outside prompt queues/history; `e-tui` MUST NOT know Pi credential paths, token shapes, or OAuth endpoints. A committed Pi credential mutation MUST synchronize the affected provider in the live conversation runtime before reporting success; remote-catalog refresh is a separate best-effort result. Session replacement invalidates active authentication state and MUST report that invalidation as a terminal outcome for an in-flight flow. Recovery guidance in adapter messages MUST name an action the frontend actually performs.

## Herdr status reporting

- `e-pi` owns optional pane-local Herdr status reporting. It MUST remain disabled outside a Herdr-managed environment and MUST NOT introduce Herdr protocol or process effects into `e-tui`.
- Reports MUST distinguish active work, readiness, and pending user decisions using runtime and interaction state. Pi's settled-run lifecycle determines completion; individual turn/tool completion is insufficient. Herdr owns the distinction between seen idle results and unseen done results.
- Reporting MUST be asynchronous, bounded, best-effort, and independent of conversation success. Repeated states MUST be deduplicated without idle polling. Shutdown and returned runtime errors MUST attempt to release the reporting source after outstanding reports. Reports MUST NOT contain conversation text or authentication secrets, links, or codes.
- This integration reports status only; it MUST NOT claim native session-restore authority or modify the official Pi integration.

## Scheduling and transport bounds

- Idle runtime waits MUST remain deadline-driven; a fixed polling ticker is forbidden.
- Animation cadence MUST be at least 16 ms. Each inbound fairness turn MUST stop at 64 items or 2 ms, and visible assistant deltas MUST yield to rendering.
- DSH frames and Pi JSONL records MUST be bounded before untrusted payloads reach reducers. Pi JSONL accepts LF records, tolerates CRLF, fails closed on invalid UTF-8/JSON or oversized records, and rejects an unterminated final record. The Pi authentication helper uses inherited stdio, 256 KiB JSONL records, channels capped at 64, one active mutation, finite control/shutdown deadlines, and explicit uncertain outcomes after mid-mutation disconnect.
- Terminal frames MUST be submitted atomically. A selectable screen snapshot becomes current only after successful submission and MUST NOT enter semantic caches.

## Platform input

- Windows raw VT parsing MUST preserve bracketed paste, modified navigation, mouse input, Escape timeouts, and Backspace/Ctrl+Backspace distinctions.
- Clipboard access remains adapter-owned. Shared input receives normalized text or provider-neutral image data; paste and image blocks MUST preserve prompt-part order.
