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

## Verification

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
