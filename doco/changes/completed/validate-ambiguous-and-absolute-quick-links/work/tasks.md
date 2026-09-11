# Execution tasks

- [x] 1.1 Refactor quick-link discovery around explicit URI, absolute-path, and workspace-relative kinds plus ordered candidate groups.
  - Design: [implementation](implement.md#3-apis-and-data-model)
  - Acceptance: Discovery emits bounded longest-first local-path hypotheses at supported soft wrappers, preserves URI behavior, and performs no filesystem I/O.

- [x] 1.2 Resolve grouped validation results and annotate selected targets at shared Chinese boundaries.
  - Dependencies: 1.1
  - Design: [resolution](implement.md#43-group-resolution), [annotation](implement.md#44-annotation)
  - Acceptance: Each group emits at most one target, requires existence when ambiguous or absolute, preserves unambiguous relative missing policy, deduplicates after resolution, and inserts only the longest applicable tag at an overlapping occurrence.

- [x] 2.1 Implement equivalent native absolute-path and grouped validation in the Pi and DSH adapter ports.
  - Dependencies: 1.1
  - Design: [adapter validation](implement.md#42-adapter-validation)
  - Acceptance: Existing native absolute files and directories inside or outside cwd pass; missing, inaccessible, foreign-platform, Windows UNC, and device namespace paths fail; relative containment and symlink-escape rejection remain intact; all I/O stays in the existing blocking effect path.

- [x] 3.1 Add focused frontend and adapter regression tests for hypothesis selection, absolute validation, bounds, stale results, and rendering boundaries.
  - Dependencies: 1.2, 2.1
  - Acceptance: The reported Chinese-parenthetical example and the complete-only, both-existing, neither-existing, outside-cwd absolute, URI, escape, duplicate, overlap, and platform-rejection cases are covered without external configuration.

- [x] 4.1 Synchronize the delivered quick-link contracts into current Doco documents and run final focused verification.
  - Dependencies: 3.1
  - Acceptance: The current [presentation contract](../../../../specs/presentation.md) and [runtime/adapter contract](../../../../specs/runtime-and-adapters.md) state the delivered behavior; `cargo test -p e-tui --lib link_copy`, `cargo test -p e-pi --lib path_completion`, `cargo test -p e-dsh --lib path_completion`, `cargo fmt --all --check`, and `doco check validate-ambiguous-and-absolute-quick-links` pass, or any unavailable check and limitation is recorded below.

## Verification

- `cargo test -p e-tui --lib link_copy` — passed (13 tests).
- `cargo test -p e-tui --lib quick_links` — passed (17 tests, including controller/render integration).
- `cargo test -p e-pi --lib path_completion` — passed (3 tests).
- `cargo test -p e-dsh --lib path_completion` — passed (2 tests).
- `cargo check -p e-pi --bin pie && cargo check -p e-dsh --bin dshe` — passed.
- `cargo fmt --all --check` — passed.
- `doco check validate-ambiguous-and-absolute-quick-links` — mechanical checks passed; remaining warnings are false-positive local-reference checks for documented source/example paths.
