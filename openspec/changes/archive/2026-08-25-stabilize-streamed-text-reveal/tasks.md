## 1. Characterize Stable Wrapping and Reveal Clocks

- [x] 1.1 Add focused `wrap.rs` tests for a stable streaming frontier across Latin words, deferred spaces, CJK punctuation, hard newlines, and over-wide atoms.
- [x] 1.2 Add deterministic reveal tests that characterize the current streaming fade stall, later append, simultaneous reveal/fade deadlines, and final restoration frame.
- [x] 1.3 Add Preview UI/state characterization for wrapped-row pacing, target revision preservation, resize, and transcript-cache isolation.

## 2. Implement Stable Transcript Admission

- [x] 2.1 Add a shared width-aware stable-prefix helper to `wrap.rs` using the production UAX #14 tokenization and grapheme boundaries.
- [x] 2.2 Extend transcript reveal state with admitted-target progress plus 100ms idle and 300ms maximum hold deadlines.
- [x] 2.3 Reconcile transcript Markdown against the stable frontier, flush on `streaming = false`, and keep complete semantic/copy/Reading content unchanged.
- [x] 2.4 Mark only the affected transcript suffix dirty when admission advances or a held tail is force-released.

## 3. Separate Content and Fade Scheduling

- [x] 3.1 Refactor reveal groups to age on an independent 16ms fade deadline while preserving the static foreground profile and style semantics.
- [x] 3.2 Keep streaming tracks alive without deadlines after their fade drains, restart them on later admitted content, and retire finite completed tracks safely.
- [x] 3.3 Compose per-track admission, content, and fade deadlines and ensure delayed ticks perform at most one bounded step per due class.
- [x] 3.4 Update frontend and runner scheduler tests for independent clocks, coalesced simultaneous work, and idle shutdown.

## 4. Convert Preview to Wrapped-Row Reveal

- [x] 4.1 Add a Preview-specific line track that stores semantic progress, wrapped-row boundaries, row fade groups, and independent reveal/fade deadlines.
- [x] 4.2 Move Preview pacing after shared wrapping and before scrolling/centering, treating non-empty wrapped rows as pacing units and attaching structural blank rows.
- [x] 4.3 Preserve common-prefix progress on same-target revisions, restart on identity changes, and remap semantic progress without reset on resize.
- [x] 4.4 Extend TestBackend regressions across long logical lines, styled tool/diff/Markdown rows, final fade, zero-rate mode, and transcript-cache isolation.

## 5. Rename and Validate Preview Configuration

- [x] 5.1 Replace `preview_chars_per_second` with `preview_lines_per_second` in canonical config and set the embedded default to 30.
- [x] 5.2 Update the Settings row label/description/application and config/settings tests for the new row unit, default, range, zero behavior, and obsolete-key ignore behavior.
- [x] 5.3 Update all code, examples, and tests to use the renamed Preview line-rate field without changing transcript character rate.

## 6. Documentation and Validation

- [x] 6.1 Update `docs/client.md`, `docs/design.md`, and `AGENTS.md` in English with stable admission, Preview row pacing, independent fade deadlines, defaults, and performance constraints.
- [x] 6.2 Run scoped wrap/reveal/config/settings/transcript/Preview/scheduler tests and address regressions.
- [x] 6.3 Run `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets`.
