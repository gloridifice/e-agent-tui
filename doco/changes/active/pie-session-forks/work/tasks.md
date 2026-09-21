# Execution tasks

- [x] 1.1 Implement native fork/clone command orchestration
  - Design: [implementation](implement.md)
  - Acceptance: Commands select/mutate/refresh in order, preserve native cancellation, and send optional messages only to confirmed replacements.
- [x] 1.2 Add ancestry discovery and a provider-neutral resume tree
  - Design: [implementation](implement.md)
  - Acceptance: Parent-first incremental trees retain stable IDs, selection, ages, search ancestry, orphan visibility, and cycle safety; flat callers remain compatible.
- [ ] 2.1 Verify integration and synchronize current documentation
  - Dependencies: 1.1, 1.2
  - Acceptance: Build, scoped existing tests, formatting, Clippy, and isolated manual checks are recorded honestly; documented interfaces are synchronized without adding or modifying tests.

## Verification

- `cargo check -p e-pi` and `cargo build -p e-pi --bin pie` passed.
- `cargo test -p e-pi --test architecture`: 2 passed.
- `cargo test -p e-pi --lib session_index::`: 6 passed.
- `cargo test -p e-tui --lib resume`: 12 passed.
- `cargo test -p e-pi --lib adapter::`: 48 passed, 3 failed. The failures are existing exact command-count assertions in `skill_commands_are_kept_out_of_the_integrated_command_catalog`, `auth_companion_controls_are_hidden_and_enable_logout`, and `reload_uses_native_companion_then_replaces_catalog_before_releasing_work`; the newly advertised fork/clone commands change those counts. Tests were not modified under repository policy. Updating these assertions requires user authorization; overall verification remains open.
- `cargo fmt --all --check` and `git diff --check` passed after formatting.
- `cargo clippy --workspace --all-targets` completed with warnings. A final `cargo clippy -p e-pi --lib` completed with only warnings from existing e-tui code.
- `doco check pie-session-forks` passed mechanical checks, reporting the remaining verification task.
- An external temporary driver linked to the built e-pi/e-tui libraries exercised native Pi 0.85.1 RPC in an isolated session directory. A temporary input hook recorded messages without contacting a model. Fork and clone each delivered exactly their intended trailing message to a new session, preserving embedded spaces/newlines. Wire inspection showed replacement, state/history/catalog refresh, then prompt admission. No-argument editor behavior, local picker cancellation, native hook cancellation, and an unsaved-session failure were observed; cancelled/failed messages were absent from the capture.
- The same driver loaded native parent metadata through SessionLoader and rendered the actual shared resume page to a Ratatui buffer. Nested branches, siblings, orphans, cycles, and search ancestry were inspected. A terminal capture of that rendered text grid was visually inspected; this is a layout check, not a full interactive pie end-to-end or theme-color check. Real user session/config files were not used.
- Current architecture, session/presentation contracts, and Pi user guidance were synchronized. Existing unrelated composer/layout edits were preserved. No repository tests were added or modified.

