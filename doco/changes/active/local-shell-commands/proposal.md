<!-- doco:lifecycle v=1 created-at=2026-10-08T09:02:24Z completed-at=- archived-at=- -->
# Local shell commands

## Purpose
Run an explicit `!`-prefixed composer submission locally without making an agent
request, while retaining complete terminal output behind a compact message view.
The standalone Ferra shell-output prototype was approved.

## Scope and acceptance
- Both `pie` and `dshe` execute the text after the initial `!` in the current
  workspace. Shell submissions do not enter model prompts or prompt queues.
- Commands run asynchronously with noninteractive stdin. Capture complete stdout
  and stderr, report exit/failure/cancellation, and allow interruption through the
  existing cancel action. Reject empty commands and image attachments locally.
- One canonical terminal composite contains the command/status and raw output.
  Ordinary messages show at most five width-wrapped output rows plus a hidden-row
  hint. Preview retains complete output. Reading expands only the focused terminal
  message and folds it again when focus leaves.
- Existing Reading, Preview scrolling, complete-source copy, and Unicode wrapping
  remain authoritative. Draft sessions must not expose the prior conversation.
- Keep process/filesystem work in adapters and release frontend guards before it.
  Discard stale completions and clean up child processes on exit/session clearing.
- Update current contracts, README, and localized help. Run scoped existing tests,
  workspace formatting/Clippy, and a real-terminal smoke check. Do not add or
  modify tests, change the bridge wire contract, or persist local output in native
  agent history. Interactive shells/PTY applications and output streaming are not
  part of this delivery.

## Result
Pending — implementation authorized; completion and archive are not requested.
