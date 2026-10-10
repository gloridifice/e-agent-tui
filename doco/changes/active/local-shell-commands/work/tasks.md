# Tasks

- [x] 1.1 Implement local shell admission, canonical state, and adapter effects.
  Acceptance: ! input never reaches the model; complete output, cancellation,
  stale-result rejection, and draft/session lifetime are handled outside UI locks.
- [x] 1.2 Implement five-row folding, complete Preview, and focused Reading expansion.
  Dependencies: 1.1
  Acceptance: resizing uses shared wrapping; only the focused terminal message
  expands; source/copy and other message folding remain unchanged.
- [x] 2.1 Synchronize current contracts and user-facing documentation/help.
  Dependencies: 1.1, 1.2
  Acceptance: docs describe the delivered local, noninteractive workflow only.
- [x] 2.2 Validate formatting, Clippy, builds, scoped existing tests and terminal behavior.
  Dependencies: 2.1
  Acceptance: record actual results and any deferrals; do not complete/archive.

## Follow-up: command mode and bounded output

- [x] 3.1 Implement command-mode styling, attached/fixed/folded presentation,
  bounded whole-line tail retention and updated current contracts/help.
  Acceptance: deliver the follow-up presentation, input and retention contracts
  without changing native history or unrelated in-progress work.
- [x] 3.2 Validate builds, scoped existing tests and real-terminal behavior;
  leave completion/archive unchanged. Do not add or modify tests.
  Dependencies: 3.1
  Acceptance: verify styling, state transitions, tail limits and retained Reading
  output, and record actual results and any blocked scenarios.
  Blocked: none
  Native agent-completion scenarios passed with the normal release build; the
  pre-existing debug-build assertion remains outside this change.

## Follow-up verification

- `cargo check --workspace`, both adapter binary builds, `cargo fmt --all --check`
  and `git diff --check`: passed.
- `cargo clippy --workspace --all-targets`: passed with existing warnings; no
  diagnostics in the shell modules or new viewport code.
- Existing e-tui scopes passed: input (106), transcript (8), Reading (7),
  controller (71), Preview region (7), command tokens (6), main pane (54).
  Main pane explicitly skipped the previously reproduced baseline failure
  `pending_submissions_render_before_any_agent_echo`. No tests changed or added.
- Real Windows pie/PowerShell checks passed: bare/spaced `!` activates command
  mode; executables and `!` use Coral; the bottom rule shows `command`; narrow
  input wraps without losing it. An 80-line command displayed exactly rows
  17–80. Running output remained hidden on one bottom row; cancellation retained
  partial output. Sending a user prompt folded prior command output immediately.
- A 2100-line result retained 2000 lines with truncation metadata, starting at
  line 101 in Reading. A byte-limited result retained 253 complete lines, starting
  at line 48. Reading exit restored the normal display cap. A new-session draft
  ran a spaced command locally. Captures remain in the ignored shell-followup
  prototype directory under target.
- With the user-authorized openai-codex/gpt-6-luna model, the normal release build
  passed both live orderings: a command settled during a 200-line assistant reply
  folded on agent completion; a 20-second command outlived a short assistant reply,
  remained attached while scrolling history, then settled at the transcript tail
  with its output visible. Preview retained output after message-pane folding.
- Debug builds still encounter the pre-existing UsageCost assertion at line 218
  of the [session reducer](../../../../../crates/e-tui/src/runtime/state/session.rs),
  with both Qwen and gpt-6-luna. Logs were retained; the assertion was not changed
  or disabled in source. `cargo build --release -p e-pi --bin pie` passed.
  Live DSH and Unix execution remain unrun. All headless sessions created here
  were closed.

## Original delivery verification

- `cargo check --workspace`: passed.
- `cargo build -p e-pi --bin pie -p e-dsh --bin dshe`: passed.
- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets`: passed with existing warnings; no shell-module diagnostics.
- Existing e-tui scopes passed: composer/input (74), Reading (7), controller (71),
  effect (3), executor (3), transcript (8), Preview (19), and remaining main-pane
  cases (54 after explicitly skipping the baseline failure listed below).
- Existing DSH runtime-port cases: 4 passed. Architecture: 15 passed, 1 baseline
  failure. No tests were added or modified.
- Real pie/PowerShell terminal checks: 60-line output folded to five rows plus a
  55-row hint; Preview reached line 60; focused Reading scrolled to line 60.
  An 18-line draft message expanded completely and folded to five plus 13 when
  focus moved away. Ordered stdout/stderr capture and exit 7 were visible.
  Escape stopped a 90-second command and retained its partial output. A new-session
  draft ran pwd without materializing a session; bare ! restored input and showed
  a local error. Startup output remained visible after native attachment.
- Saved normal and Reading SVG captures are in target/prototype. Headless sessions
  were closed. Live DSH service execution and Unix process behavior were not run;
  both adapters built, share the same frontend policy, and have adapter-owned runners.

## Deferred baseline issues

Both failing tests were reproduced against a temporary clean HEAD archive:

- pending_submissions_render_before_any_agent_echo: the existing test expects
  uppercase [Skill], while unchanged production renders lowercase [skill].
- runners_share_selection_policy_and_publish_only_after_submission: its existing
  source-string lookup at architecture.rs:222 fails on the baseline too.

An early slash-command smoke attempt was affected by Git Bash argument conversion,
which sent a path instead of /new and encountered the existing UsageCost debug
assertion. It was repeated successfully with MSYS_NO_PATHCONV=1. This unrelated
native usage reducer issue and the two baseline tests are not changed here.

Lifecycle remains active; completion/archive were not requested.
