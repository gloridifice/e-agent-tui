# Execution tasks

Implementation was authorized and delivered. The change remains active; completion and archive are intentionally separate operations.

- [x] 1.1 Add shared pure path-completion policy and its focused tests
  - Design: [implementation](implement.md#3-apis-and-data-model)
  - Acceptance: `e-tui::path_completion` owns name eligibility, case-insensitive substring matching, ordering, and result truncation without filesystem effects. Shared tests cover the policy matrix and boundaries.

- [x] 1.2 Route both adapter completion functions through the shared policy
  - Dependencies: 1.1
  - Design: [algorithm and rules](implement.md#4-algorithms-and-rules)
  - Acceptance: Both adapters use the matcher before file-type probes and use shared finalization. Their completion functions contain no independent matching, ordering, or result-truncation policy. Existing native path handling, scan bound, directory navigation, and blocking-task execution remain unchanged. Consolidate the preliminary duplicate test matrices into shared coverage and small adapter filesystem checks.

- [x] 2.1 Verify integration and synchronize delivered current contracts
  - Dependencies: 1.2
  - Design: [verification and documentation impact](implement.md#6-verification-and-documentation-impact)
  - Acceptance: Scoped completion and architecture checks pass; both adapters produce the directory named `readme` and the README Markdown file for the requested example. Current interaction and runtime contracts reflect the delivered behavior and ownership. Record actual commands, outcomes, and any deferrals without completing or archiving the package automatically.

## Verification

Verification:

- `cargo fmt --all --check` — passed.
- `cargo test -p e-tui --lib path_completion` — passed (6 tests, including shared matching/order/limit policy and frontend completion behavior).
- `cargo test -p e-dsh -p e-pi --lib path_completion --no-fail-fast` — passed (3 DSH tests, 4 Pi tests; both adapter integration checks return the expected directory and file for `@ea`).
- `cargo test -p e-dsh -p e-pi --test architecture --no-fail-fast` — passed (16 DSH architecture tests, 2 Pi architecture tests).
- `doco check share-path-completion-policy` — passed mechanical checks.

The preliminary prefix-only baseline command was run before implementation and failed as expected; it is retained in Git history rather than repeated as a delivery check.
