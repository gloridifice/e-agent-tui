# Implementation design

## 1. Baseline and goals

- [Transcript rendering](../../../../../crates/e-tui/src/ui/transcript.rs) renders User cards as plain text between rules, and Attachment cards as plain text in their existing shell. Folding currently applies to activities, not user messages.
- `presentation.rs` materializes only assistant Markdown blocks into `MarkdownLayoutRegistry` in `transcript_layout.rs`. `render.rs` provides the shared parser, styles, width-aware structural layout, and provenance.
- `reading.rs` keeps each card as a single full-source Reading block, but its fallback Preview is plain text. Normal Preview intentionally does not follow user cards automatically; explicit Reading selection is the existing full-message Preview entry point.
- The pending first submission on a new-conversation draft is rendered directly by the [transcript viewport](../../../../../crates/e-tui/src/ui/transcript/viewport.rs) before entering the transcript cache; materialize its Markdown at that viewport width too.
- Live submission and replay already share ContentCard surfaces and complete copy sources. User attachments use CardRole::Attachment; their labels remain part of the displayed source.
- Current presentation contracts prohibit independent table/code folding. The approved exception is whole-user-message folding, not a change to the shared Markdown renderer.
- Existing uncommitted changes are confined to the unrelated automated-demo-recording Doco package and must remain untouched.

## 2. Overall approach

Keep this change in e-tui presentation and Reading projection. Materialize complete user-card Markdown through the existing render-only registry at the actual card body width. Retain the card's one full-message copy identity rather than allocating persistent per-Markdown-block copy units. Use the existing transcript cache for rendered head/tail output; no new state in semantic transcript or adapters.

Use one card-geometry helper for materialization and rendering. Preserve ruled User shells and filled Attachment shells. Use each card's existing ordinary-text tone as the Markdown text tone, while preserving Markdown semantic accents and code backgrounds.

## 3. APIs and data model

Extend MarkdownLayoutRegistry with a card-specific materialization entry point using ContentCard, Theme, and RenderOptions. Render into temporary unit storage, remap retained layout provenance to the existing whole-card unit, and store complete styled rows under its DisplayId. Source/width invalidation and theme/config invalidation follow the existing registry lifecycle. Remove a pending card's layout when its authoritative echo replaces it.

No persistent format, provider payload, public interaction, concurrency, or effect changes. The user-card renderer reads the complete materialized layout. Reading still exposes one block and the complete copy_source; User and Attachment cards select full Markdown Preview instead of plain text.

## 4. Algorithms and rules

1. Resolve body width from the existing shell and padding, with a minimum rendering width of one cell.
2. Parse complete Markdown using current transcript render options. Wrap resulting styled rows using the shared grapheme-safe wrapper; keep code fill backgrounds within the body.
3. If the body has more than 20 rows, replace the middle with a muted marker counting exactly total minus 20. Preserve the first and last 10 rows without reparsing fragments.
4. Add the unchanged shell/header outside the folding budget. Clip the marker to one body row in extremely narrow panes.
5. Recompute on resize, including list/table geometry; do not bypass folding in Reading mode.
6. Preview and whole-message copy use the complete semantic source, not the folded output. No links or navigation are invented from the marker.

Empty/whitespace-only Markdown uses the shared renderer's empty-body behavior. At 21 rendered rows the marker replaces one row even though total displayed body height remains 21. Markdown margins and table/code chrome count as body rows.

## 5. Fixed decisions and discretion

Fixed: rendered-row counting; 20-row threshold; 10/marker/10 shape; hidden-row count; complete Markdown Preview; no inline expansion; unchanged input and copy source. Local helper names and equivalent cache mechanics are discretionary. No blocking questions remain.

## 6. Verification and documentation impact

Run scoped existing main-pane, Reading, Markdown layout/rendering, and submission tests plus `cargo check -p e-tui`. Do not introduce or modify tests. Review threshold arithmetic, width changes, card-shell retention, and full-source copy/Preview paths directly where existing tests do not cover the new behavior. Report that limitation rather than claiming new automated coverage.

Update only the [presentation contract](../../../../specs/presentation.md) for the approved user-message contract and exception. Architecture ownership and interaction keys do not change, so architecture, README, help, and ADR updates are unnecessary. Run `doco check user-message-markdown-folding`; keep the package active after execution.
