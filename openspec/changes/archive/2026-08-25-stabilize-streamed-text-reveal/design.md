## Context

The presentation-only reveal implementation in `e-tui::reveal` currently has one logical deadline per lane. Transcript Markdown is clipped by a rendered-grapheme cursor and later wrapped by the shared greedy UAX #14 wrapper. Extending the final wrap token can therefore move already painted text. `RevealTrack::has_pending_work` also admits fade-only work only after `finite` becomes true, so an open stream whose visible queue is empty has no deadline and leaves its newest group dark until settlement. Preview applies the same grapheme transform before wrapping, although the requested unit is now a wrapped terminal row.

Complete transcript and Preview semantics must remain immediately available to copy, Reading View, caches, and reducers. The frontend must remain kernel-neutral and the runner must remain event-driven with no fixed polling ticker.

## Goals / Non-Goals

**Goals:**

- Admit only a stable rendered prefix of a live assistant tail to the existing character-paced transcript lane.
- Bound tail latency with idle and maximum hold deadlines and flush immediately when the existing streaming lifecycle settles.
- Age foreground fade groups on an independent frame clock even when no content unit is waiting and the source remains open.
- Reveal Ready Preview by wrapped display row at 30 rows/s by default.
- Preserve incremental transcript suffix splicing, Preview cache isolation, resize behavior, complete semantic source, and earliest-deadline scheduling.

**Non-Goals:**

- Delaying event reduction, copy source, Reading documents, Preview resolution, or bridge acknowledgements.
- Freezing a permanent online line layout that differs from the canonical shared wrapper.
- Changing transcript reveal from graphemes to rows.
- Adding a bridge protocol event; `TranscriptBlock.streaming` remains the settlement signal.
- Making the hold timeouts user-configurable in the first implementation.

## Decisions

### 1. Gate transcript reveal with a stable grapheme frontier

Extend the transcript track reconciliation input from only a complete rendered signature to also include an `admitted` grapheme frontier. Rendering and copy continue to materialize complete Markdown, but `RevealTrack` may reveal only `0..admitted` until more content becomes stable.

The frontier is calculated from the same styled logical lines and width used by the transcript cache. Every completed logical line is admitted. On the final streaming logical line, a helper in `wrap.rs` uses the existing UAX #14 tokenization and greedy wrapping rules to retain the open trailing wrap atom, its deferred separator, and only the incomplete final row of an over-wide atom. Closed tokens and complete hard-wrapped rows are admitted immediately. A source ending in an explicit hard newline admits the preceding line.

This token/partial-row holdback is preferred to withholding the entire final display row: it prevents whole-word relocation without adding paragraph-length latency. It is preferred to a second ad-hoc whitespace splitter because CJK punctuation, kana, emoji, numeric punctuation, and grapheme boundaries must match the production wrapper.

The admitted frontier is monotonic for append-only source. Rendered-prefix divergence still clamps progress to the actual common grapheme prefix as today. A timeout-flushed atom that later grows may canonically reflow; this unavoidable ambiguity is bounded to the force-flushed tail rather than solved by retaining a permanently noncanonical online layout.

### 2. Use idle and absolute hold deadlines

A streaming track records when the currently held rendered tail first appeared and when its visible signature last changed. It flushes the held tail when any of these occurs:

- the signature has not changed for 100ms;
- the same held tail has waited 300ms in total;
- reconciliation reports `finite = true` from the existing end/settle lifecycle.

Only a rendered signature growth or divergence resets the idle deadline; unrelated frames and source events that add no rendered grapheme do not. When a stable boundary advances naturally, the new frontier becomes available immediately and timeout state applies only to the remaining tail. Timeout expiry admits the current target into the ordinary character queue; it does not bypass `message_chars_per_second`.

Both timeout values are named constants and deterministic tests use explicit `Instant` values. They are intentionally not persisted settings until observation shows a user-facing tuning need.

### 3. Separate admission, content pacing, and foreground fading

Refactor each paced lane to expose the minimum of independent deadlines:

- admission deadline: transcript holdback only;
- reveal deadline: next grapheme batch or Preview row batch;
- fade deadline: the next 16ms age step for visible fade groups.

A reveal step appends a group at fade age zero. A fade step ages groups that existed before the step; a group leaving `TEXT_FADE_WEIGHTS` uses its original semantic foreground and no longer needs animation work. If reveal and fade are due together, old groups age first and newly revealed groups remain at profile index zero for their first painted frame.

Fade work does not depend on `finite`. A live track can therefore drain to an idle, fully colored state while remaining available for a later append. A later unit starts a new reveal/fade group. A transcript track is retired only when the stream is finite, the target and admitted/revealed frontiers are complete, no holdback exists, and no fade group remains active.

This replaces the current global `drain` behavior. It is preferred to immediately restoring the suffix because every profile color must receive a paint opportunity, and preferred to a fixed global ticker because idle lanes must have no deadline.

### 4. Give Preview a row-specific track

Preview wrapping moves before pacing:

`PreviewContent -> semantic styled lines -> shared wrap -> row reveal/fade -> viewport/scroll/centering`.

A `PreviewLineTrack` owns a semantic rendered signature, a revealed semantic frontier, current wrapped-row boundaries, fade groups, and reveal/fade deadlines. One pacing unit is one non-empty wrapped display row. Structural blank rows attach to the following non-empty row (or the preceding row at the end) so a tick always causes visible progress. The first row may appear immediately. Higher rates batch `ceil(rate * 16ms)` rows, while 30 rows/s schedules one row every 1/30 second.

The track stores semantic grapheme progress rather than only a row count. Width changes recompute row boundaries without resetting target identity or exposing graphemes beyond the existing semantic frontier. A resize may temporarily place that exact frontier inside a newly wrapped row; the next row tick advances to the next current boundary. Same-target revisions preserve the common rendered prefix, while identity changes still reset Preview reveal and scroll according to existing policy.

A separate track is preferred to adding a unit mode to the existing transcript track because transcript admission and Preview width-dependent row boundaries have different invariants. Shared helpers retain the pacing batch calculation, fade profile, styled-grapheme indexing, and deadline composition.

### 5. Rename the Preview rate by unit

Replace `preview_chars_per_second = 300` with `preview_lines_per_second = 30` in the canonical `Config`, embedded default TOML, Settings row, and documentation. The validated rate remains an integer in `0..=1024`; zero reveals all rows immediately, and rates above one row per frame use bounded batches.

The existing known-key overlay ignores the obsolete user key. No numeric migration is attempted because character and wrapped-row rates depend on content and terminal width and have no sound conversion.

### 6. Preserve earliest-deadline and cache boundaries

`TuiApp::reveal_deadline` remains the minimum across transcript and Preview tracks, but each track now returns the minimum of its own admission, reveal, and fade clocks. `tick_reveals` advances only due work and requests one coalesced animation frame. No fixed ticker is introduced.

Admission, reveal, and fade changes mark the active transcript message as reveal-dirty and retain suffix splicing from the earliest changed message. Preview row/fade changes dirty only the Preview frame and never invalidate `TranscriptRenderCache` or semantic Preview cache entries.

## Risks / Trade-offs

- **[A timeout-flushed token later grows]** Canonical greedy wrapping may still revise that final row. → Bound the case to the forced tail, use natural UAX boundaries whenever available, and cover late continuation explicitly in tests.
- **[Stable-frontier logic diverges from wrapping]** A duplicate tokenizer would reintroduce movement for CJK or punctuation. → Put the helper in `wrap.rs` and share its private tokenization and row builder rules.
- **[Three deadlines make track state harder to reason about]** Stale deadlines could cause polling or skipped frames. → Centralize `next_due`, use explicit deterministic instants, and test every transition including simultaneous deadlines and idle shutdown.
- **[Preview resize crosses a revealed row]** Preserving semantic progress can produce one partial remapped row. → Never hide or expose semantic graphemes solely because of resize; complete the current row on the next line step.
- **[Row reveal materializes more Preview layout]** Full wrapping is needed to know row boundaries. → Preview content is already bounded/cached, visible materialization remains bounded, and transcript caching is unaffected.
- **[More fade frames for slow streams]** Every arriving group now receives bounded fade work. → The profile is only two frames, deadlines stop immediately after restoration, and simultaneous work coalesces into one frame.

## Migration Plan

1. Add deterministic wrapping-frontier and reveal-clock tests before changing presentation paths.
2. Refactor transcript fade scheduling and add admission state while retaining character clipping.
3. Add Preview row tracking and move Preview pacing after wrapping.
4. Rename the config/default/Settings field and update scoped tests.
5. Update runner deadline tests, TestBackend regressions, and architecture documentation.
6. Run scoped tests, then `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets` because the change crosses frontend and runner scheduling.

Rollback is a source revert. Config files containing the new row-rate key are safely ignored by older known-key overlays; files containing the obsolete character-rate key are ignored by the new overlay.

## Open Questions

None. The initial constants are 100ms idle hold, 300ms absolute hold, 16ms fade cadence, and 30 Preview display rows per second.
