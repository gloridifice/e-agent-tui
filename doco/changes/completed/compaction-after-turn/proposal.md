<!-- doco:change mode=proposal-only -->
<!-- doco:lifecycle v=1 created-at=- completed-at=2026-09-29T02:21:45Z archived-at=- -->
# compaction-after-turn

## Purpose

Ordinary prompts submitted during compaction currently use ASAP delivery and can enter the ongoing run. Default these submissions to the existing local after-turn queue so the current task can finish first.

## Scope and acceptance

- Shared `e-tui` policy covers live manual and automatic compaction events from both adapters. Track compaction identity independently of thinking and session status; matching success, failure, or cancellation ends it. History replay must not activate or clear live compaction state, and session replacement clears it.
- Ordinary sends, including remapped ASAP sends and model-marked prompts, use `AfterTurn` when submitted during compaction. Explicit after-turn sends remain unchanged. New-session drafts and slash commands retain their existing semantics; older queued prompts are not reclassified.
- Compaction is not idle. After-turn prompts remain local until the current run, compaction, and active commands settle, with existing model-restoration barriers preserved. Compaction completion alone must not release them while the run continues.
- Reuse existing queue display, ordering, and cancellation behavior. Do not introduce a force-ASAP binding, configuration setting, adapter wire change, or backend queue.
- Update the current interaction contract, help tips in both languages, and README usage guidance.
- Validate with scoped existing queue, model, compaction, and key-mapping tests plus compilation of both frontends. Do not add or modify tests.

## Verification

- `cargo test -p e-tui --lib runtime::controller::`: passed (71 tests).
- `cargo test -p e-tui --lib compaction`: passed (7 tests).
- `cargo test -p e-tui --lib prompt_queue`: passed (3 tests).
- `cargo test -p e-pi --lib compaction`: passed (7 tests).
- `cargo check -p e-dsh -p e-pi --bins`: passed.
- `rustfmt --check --edition 2021 --config skip_children=true` on the six changed Rust files: passed.
- `git diff --check` and `doco check compaction-after-turn`: passed.
- No tests were added or modified. Live backend interaction was not exercised; the checks above cover existing behavior, not dedicated new regression cases.

## Result

Delivered: during live compaction, ordinary sends use the existing after-turn queue and wait for the run and commands to settle. The interaction contract, help, and README are updated. Scoped existing tests, both frontend builds, formatting, and mechanical checks passed; live backend behavior was not exercised. No tests or adapter wire changes were added.
