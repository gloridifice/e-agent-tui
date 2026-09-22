# Execution tasks

- [x] 1.1 Implement fallback scheduling, lifecycle cancellation, and countdown projection
  - Design: [implementation](implement.md)
  - Acceptance: Native recovery runs first; five bounded delays update one row; interruption, success, replacement, and exhausted/rejected retries terminate correctly without replaying original tasks.
- [x] 1.2 Add a default-on live settings switch
  - Dependencies: 1.1
  - Acceptance: Behavior settings expose `error_auto_retry`; disabling cancels pending fallback but preserves native retry configuration; persistence and reload use existing config paths.
- [x] 2.1 Validate and synchronize delivered contracts
  - Dependencies: 1.1, 1.2
  - Acceptance: Run scoped existing checks, record outcomes and limitations, update current affected contracts, and leave the change active.

## Verification
- `cargo check -p e-pi -p e-dsh`: passed.
- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --message-format=short`: passed with warnings in existing code; no warning in the new retry module.
- `cargo test -p e-tui --lib settings::`: 14 passed.
- `cargo test -p e-tui --lib config::`: 11 passed.
- `cargo test -p e-tui --lib i18n::`: 4 passed.
- `cargo test -p e-tui --lib projection::`: 11 passed.
- `cargo test -p e-tui --lib runtime::state::`: 14 passed.
- `cargo test -p e-pi --test architecture`: 2 passed.
- `cargo test -p e-pi --lib adapter::`: 48 passed, 3 existing command-catalog assertions failed. Reproduced exactly the same failures on unchanged HEAD `65711bb` in a temporary detached worktree, then removed that worktree. The failures are `reload_uses_native_companion_then_replaces_catalog_before_releasing_work`, `auth_companion_controls_are_hidden_and_enable_logout`, and `skill_commands_are_kept_out_of_the_integrated_command_catalog`. The final adapter run with these three baseline failures explicitly skipped passed all remaining 48 tests. No tests were added or changed.
- `git diff --check`: passed. Changes remain scoped to fallback recovery, shared presentation/configuration, and their current documents; no overlapping active change was executed.
- Reviewed native RPC acceptance/settled semantics against the installed Pi implementation, deadline arithmetic and five-attempt exhaustion, cancellation suppression, stale response correlation, live config synchronization, and explicit activity lifecycle ownership.
- Deferred: real-provider end-to-end recovery, interactive visual verification, and an elapsed 81-minute backoff sequence. Existing tests do not directly exercise the new retry state machine. No new tests were added per repository policy.
- Current architecture, runtime, presentation, configuration contracts, and the user-facing settings entry were synchronized. The Doco package remains active; no completion or archive was requested.
