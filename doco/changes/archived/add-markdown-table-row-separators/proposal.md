# add-markdown-table-row-separators

## Purpose
Markdown tables separate the header from the body but leave data rows adjacent. Adopt the approved Python prototype's row separators so logical rows remain distinguishable, especially when cells wrap.

## Scope and acceptance
- Add one existing-style horizontal separator between adjacent visible logical rows in the shared `e-tui` table renderer, including collapsed tables and both width-aware and unbounded rendering.
- Keep wrapped continuation lines together. Preserve outer borders, column widths, inline formatting, atomic ownership, and raw source for copy.
- Separate the collapsed summary from the visible rows on either side without changing collapse eligibility, retained rows, hidden counts, or expansion behavior.
- Verify ordinary, wrapped, collapsed, and expanded tables with scoped Rust regression tests.

Non-goals: parser or wrapping changes, new configuration or dependencies, adapter changes, and changes to other rendering surfaces. Current architecture and documented presentation contracts remain unchanged; separator placement is an implementation detail covered by tests.

## Result
Delivered. `render_table` now emits one `├┼┤` separator between adjacent visible logical rows for both width-aware and unbounded rendering, keeps wrapped continuation lines inside a single row group, and separates the collapsed summary from the visible rows above and below it. No parser, wrap, collapse-threshold, raw-source, or adapter behavior changed, and no current document required an update.

Verified with `cargo test -p e-tui --lib render::tests::table_` (7 passed), scoped `rustfmt --check`, `git diff --check`, and `doco check`. Full workspace tests, Clippy, and production terminal smoke tests were not run; the change is intentionally limited to table row boundaries and their regression tests.
