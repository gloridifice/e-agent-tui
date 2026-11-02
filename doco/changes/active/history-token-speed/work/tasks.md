# Execution tasks

- [x] 1.1 Capture native response metrics and enclosing run boundaries
  - Design: [implementation](implement.md)
  - Acceptance: Pi emits provider-neutral metadata; recorded usage retains its own response identity, optional duration and outcome; internal turns do not close the recorded run; legacy records remain readable and DSH timings remain unknown.
- [x] 1.2 Render per-response and weighted whole-turn speed
  - Dependencies: 1.1
  - Acceptance: Only output tokens and positive corresponding durations enter speed; missing and unsuccessful coverage is explicit; coincident responses retain distinct rates; ranking and the five-second timeline scale are preserved.
- [x] 2.1 Verify and synchronize current contracts
  - Dependencies: 1.1, 1.2
  - Acceptance: Relevant existing checks pass or limitations are recorded; current presentation/storage contracts match delivered behavior; README and tests remain unchanged; the change remains active.

## Verification
- `cargo check --workspace`: passed.
- `cargo test -p e-tui --lib execution_history`: 7 passed.
- `cargo test -p e-tui --lib history`: 28 passed, including capture, history navigation/layout and legacy history checks.
- `cargo test -p e-tui --lib projection`: 11 passed.
- `cargo test -p e-pi --lib adapter`: 54 passed, rerun after the final metadata validation change.
- `cargo test -p e-pi --lib execution_history_store`: 16 passed.
- `cargo test -p e-dsh --lib execution_history_store`: 16 passed.
- `cargo test -p e-dsh --test architecture`: 13 passed; three existing runner-source assertions failed (`runners_deliver_completions_from_every_action_batch`, `runners_process_admitted_input_before_claiming_queued_prompts`, `runners_share_selection_policy_and_publish_only_after_submission`). Reproduced the identical failures from an untouched `git archive HEAD` at `63037a2`, compiling its original architecture test with `rustc --test`, the current Cargo-built toml dependency, and the archived manifest directory. No tests or runner code were changed to bypass them.
- `cargo fmt --all` and `cargo fmt --all --check`: passed; no test changes.
- `cargo clippy --workspace --all-targets`: completed with warnings in unchanged code; no new-metric warnings.
- `git diff --check`: passed. Current storage/presentation contracts synchronized; README unchanged.

Limitations: no new tests were authorized or added. No paid live model call or interactive terminal visual verification was performed. Existing checks do not provide new throughput-specific regression coverage. DSH timing and legacy-history backfill remain explicitly unsupported; missing values display unknown. The change remains active, not completed or archived.
