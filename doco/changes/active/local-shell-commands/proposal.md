<!-- doco:lifecycle v=1 created-at=2026-10-08T09:02:24Z completed-at=- archived-at=- -->
# Local shell commands

## Purpose
Run an explicit `!`-prefixed composer submission locally without making an agent
request, while retaining bounded terminal output behind a compact message view.
The standalone Ferra shell-output prototype was approved.

## Scope and acceptance
- Both `pie` and `dshe` execute the text after the initial `!` in the current
  workspace, accepting any number of spaces after `!`. Shell submissions do not
  enter model prompts or prompt queues. Command-mode input uses Preview command
  highlighting with Coral instead of Blush, a Coral `!`, and the selected shell's
  name in the bottom input rule using the model-name tone.
- Commands run asynchronously with noninteractive stdin. Capture shared stdout
  and stderr, retain a bounded tail, report exit/failure/cancellation, and allow interruption through the
  existing cancel action. Reject empty commands and image attachments locally.
- One canonical terminal composite contains command/status and retained raw output.
  Running commands attach to the bottom message row without output. Settlement
  fixes their transcript position and shows the last 64 width-wrapped output rows.
  New user messages and agent completion fold settled commands to their header.
  Reading expands only the focused terminal message; leaving restores its state.
  Preview and copy use retained output, bounded to a whole-line tail by bytes and
  lines. Discard oldest lines first and indicate truncation.
- Existing Reading, Preview scrolling, retained-source copy, and Unicode wrapping
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
