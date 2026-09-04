## 1. Normal-Mode Activity Folding

- [x] 1.1 Add a linear transcript presentation plan that folds consecutive runs over six one-row activities into first-three, localized summary, and last-three rows.
- [x] 1.2 Apply the plan consistently to cache ranges, spacing, visible layout, and provenance while keeping Reading View fully expanded.
- [x] 1.3 Add focused tests for threshold, multiple runs, hidden reasoning adjacency, live run growth, and Reading entry/exit.

## 2. Structured Tool Preview Layout

- [x] 2.1 Extend cached Preview layout metadata to distinguish wrapped tool information from terminal secondary rows.
- [x] 2.2 Clip terminal output rows without wrapping or ellipsis and pin wrapped tool information once the combined content overflows, preserving centering while it fits.
- [x] 2.3 Add renderer tests for centered short output, sticky long output, narrow clipped output, and information-only overflow.

## 3. Preview Reveal Semantics

- [x] 3.1 Track fresh-live versus page-style Preview selection intent without changing semantic target/cache identity.
- [x] 3.2 Extend `LineRevealTrack` with whole-block admission while retaining row pacing for fresh live reasoning and independent fade scheduling.
- [x] 3.3 Route non-reasoning, replay/resume, cached revisit, Reading selection, and same-target revision behavior through the required reveal mode.
- [x] 3.4 Add focused reveal and Preview integration tests for block fade, reasoning row pacing, historical/Reading page fade, revision stability, resize, and zero rate.

## 4. Streaming Markdown Stability

- [x] 4.1 Preserve the painted transcript frontier for append-only live Markdown rematerialization while retaining strict common-prefix rollback for replacement.
- [x] 4.2 Add a regression test for a streaming fenced code block whose generated line-count header changes and for final settlement.

## 5. Verification

- [x] 5.1 Run scoped `e-tui` reveal, transcript, Preview, and UI tests covering the changed modules.
- [x] 5.2 Run `cargo fmt --all --check` and validate the OpenSpec change.

## 6. Completion-Gated Folding and Separator Transparency

- [x] 6.1 Gate normal-mode activity folding on following assistant Markdown or interruption/error outcomes, without treating informational tool activities as fold triggers.
- [x] 6.2 Restore terminal-reset backgrounds for separator bar/guide roles that omit an explicit theme background.
- [x] 6.3 Add focused boundary and separator tests, update the client architecture invariant, and rerun scoped validation.

## 7. Enforced Terminal Output No-Wrap

- [x] 7.1 Preserve terminal-output row identity in Preview layout and clip those rows only at the final viewport boundary after reveal processing.
- [x] 7.2 Remove synthetic terminal-output ellipsis rows and add focused cached-layout and rendered-output regression tests.
- [x] 7.3 Run scoped Preview tests, formatting, diff checks, and OpenSpec validation.
