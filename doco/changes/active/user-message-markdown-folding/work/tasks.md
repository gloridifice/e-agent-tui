# Execution tasks

- [x] 1.1 Render and fold complete user-card Markdown at the resolved body width
  - Design: [implementation](implement.md)
  - Acceptance: Live/history User and Attachment cards retain their shells, reuse cached Markdown layout, and display complete bodies up to 20 rows or the approved 10/marker/10 window above it. Resize recomputes the window. Semantic sources and whole-card copy identity remain intact.

- [x] 1.2 Preserve complete Markdown Preview for user messages
  - Dependencies: 1.1
  - Design: [implementation](implement.md)
  - Acceptance: Reading selects complete user-message Markdown and copies complete source, including attachment-bearing prompts; transcript folding is unchanged by entering Reading and no expansion action is added.

- [x] 2.1 Validate implementation and synchronize the presentation contract
  - Dependencies: 1.1, 1.2
  - Acceptance: Relevant existing tests and frontend build checks are run and outcomes recorded; no tests are added or modified. Current presentation contracts describe delivered user-message folding without changing other Markdown surfaces. Doco validation passes and the package remains active.

## Verification

- `cargo check -p e-tui`: passed.
- `cargo test -p e-tui --lib ui::main_pane_tests::`: 55 passed, covering existing user-card shell/copy behavior, pending live/draft submissions, Reading navigation, resize, Markdown layout, and unchanged composer behavior.
- `cargo test -p e-tui --lib reading::tests::`: 7 passed.
- `cargo test -p e-tui --lib transcript_layout::tests::`: 7 passed, including width-sensitive rematerialization and grapheme-safe wrapping.
- `cargo test -p e-tui --lib render::tests::`: 38 passed, including complete long table/code/Mermaid layouts.
- `cargo test -p e-tui --lib runtime::state::session::tests::`: 4 passed, including optimistic submission/echo reconciliation.
- `rustfmt --check --config skip_children=true --edition 2021` on the seven changed Rust files: passed. No tests were added or modified.
- `git diff --check`: passed. Diff review confirmed that composer, adapters, semantic sources, and unrelated Doco changes were untouched.
- `doco check user-message-markdown-folding`: mechanical checks passed; the package remains active.
- Source-path review: folding is applied only after full Markdown layout and body wrapping; the 20-row boundary remains intact and 21 rows hide exactly one; code/table rows participate. Both card roles share width geometry, with a reserved body cell at narrow widths. Reading Preview selects complete source and neither whole-card copy identity nor sent content is replaced by folded rows. The pending first message on a new-conversation draft also materializes Markdown.
- Current presentation contract synchronized. No architecture, key-binding/help, README, persistent format, or adapter contract changes were needed.
- Limitation: existing tests do not directly exercise the new 10/marker/10 behavior or user Markdown Preview variant. Those paths were source-reviewed, not newly automated or visually exercised; no new tests were authorized. Completion and archive are deferred until requested.
