# Execution tasks

- [x] 1.1 Extend normalized usage capture and the persistent history schema
  - Design: [implementation](implement.md#3-apis-and-data-model)
  - Acceptance: Pi captures native response price; both adapters persist model,
    message-kind, and token usage records without content; DSH price remains
    absent rather than zero; existing version-1 records still deserialize.

- [x] 1.2 Return chronological records from both history stores
  - Dependencies: 1.1
  - Design: [implementation](implement.md#2-overall-approach)
  - Acceptance: ranking and record lists use one finite watermark and worker
    failures keep existing page-level error behavior.

- [x] 2.1 Add history view state, semantic Tab switching, and timeline rendering
  - Dependencies: 1.2
  - Design: [implementation](implement.md#4-algorithms-and-rules)
  - Acceptance: ranking is initial, Tab switches to a fixed five-second timeline,
    offsets are independent, summaries and model rows are correct, unknown price
    is explicit, and the timeline has none of the removed prototype chrome.

- [x] 3.1 Synchronize current contracts
  - Dependencies: 2.1
  - Acceptance: storage, interaction, and presentation specs describe delivered
    fields, boundaries, key action, and timeline behavior without duplicating
    source-level registries.

- [x] 4.1 Run focused verification and review the delivered diff
  - Dependencies: 3.1
  - Acceptance: touched code is formatted, relevant existing checks pass or
    limitations are recorded, Doco validates, and unrelated working-tree edits
    remain intact.

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo check -p e-tui -p e-pi -p e-dsh` — passed.
- `cargo test -p e-tui execution_history::tests::ingress_capture -- --nocapture`
  — passed (2 tests).
- `cargo test -p e-tui ui::region::history -- --nocapture` — passed (4 tests).
- `cargo test -p e-pi history -- --nocapture` — passed (18 tests, plus child
  process checks).
- `cargo test -p e-dsh history -- --nocapture` — passed (17 tests, plus child
  process checks).
- `cargo test -p e-pi cost -- --nocapture` — passed (3 tests).
- `cargo clippy -p e-tui -p e-pi -p e-dsh --lib --no-deps` — passed with the
  repository's existing warnings; no warning points at this change after fixing
  the timeline number formatter.
- `cargo clippy -p e-tui -p e-pi -p e-dsh --all-targets -- -D warnings` — did
  not pass because existing warnings across unrelated library and test code are
  promoted to errors.
- `cargo test -p e-tui history -- --nocapture` — 26 passed and 2 failed. Both
  failures assert the intentionally retired behavior that `history.toggle_view`
  is ignored and Tab cannot switch the page. They were not edited because the
  repository policy forbids modifying tests unless explicitly requested.
- No live-provider visual smoke run was performed; new timeline records require
  an attached Pi or DSH session. The production renderer and relevant existing
  history renderer checks compile and pass.
