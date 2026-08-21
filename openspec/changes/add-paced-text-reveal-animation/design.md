## Context

Live assistant chunks are currently reduced into the complete `TranscriptBlock` source and painted immediately. `TranscriptRenderCache` has an efficient streaming-tail splice, but the network's chunk boundaries still determine how much text appears in each frame. Preview similarly stores complete, width-independent `PreviewContent` and materializes the selected value all at once. The only periodic animation clock today is the runner's spinner/settle clock, whose cadence comes from `spinner_frame_ms`.

The requested effect has two separate concerns:

1. presentation pacing: upstream content may become semantically available immediately, but visible text advances at a bounded transcript or Preview rate;
2. suffix fading: the newest visible characters temporarily replace their semantic foreground with a configured background-to-foreground interpolation based on one static weight profile.

The implementation must remain kernel-neutral in `e-tui`, preserve full source and copy/Reading semantics, work across all styled Preview variants, and retain the event-driven scheduler and incremental transcript-cache redlines. The existing `unicode-segmentation` dependency already supplies the required grapheme boundaries.

## Goals / Non-Goals

**Goals:**

- Provide one reusable grapheme-aware operation for clipping styled lines to reveal progress and fading the visible suffix.
- Decouple live assistant Markdown paint rate from bridge chunk rate, with a default maximum of 120 characters/s.
- Reveal every Ready Preview content kind through the same operation at an independent default maximum of 300 characters/s.
- Preserve Markdown/diff/ANSI/theme foreground distinctions and every non-foreground style attribute.
- Keep reveal state as presentation-only sidecar state and update only the affected transcript suffix or Preview pane.
- Add strictly validated, immediately applied persisted settings for the background reference and both rates.

**Non-Goals:**

- Delaying event reduction, model state, copy source, Reading View, Preview resolution, or bridge acknowledgements until text is painted.
- Animating user messages, activity rows, status text, Input Pages, help, loading/error labels, or historical transcript replay.
- Making the fade-weight profile user-configurable; it is intentionally one static code array whose length can be changed later.
- Replacing semantic surface backgrounds with `background_color`; the new color is the fade interpolation origin only.
- Re-parsing a progressively sliced Markdown source or exposing Markdown control markers as reveal characters.
- Changing the WebSocket protocol or Node bridge.

## Decisions

### 1. Model reveal as a reusable logical track plus a styled-line transform

Add an `e-tui` leaf module (for example `reveal.rs`) with:

- `TEXT_FADE_WEIGHTS: &[f32] = &[0.217, 0.53]`, ordered newest-to-oldest;
- a `RevealTrack` containing target identity, the normalized rendered-text signature, revealed grapheme count, fade-drain age, finite/streaming status, and next due instant;
- a function that accepts fully semantic `Vec<Line<'static>>`, a track snapshot, configured background RGB, and plain-color state, then returns only the visible styled prefix with the fade applied.

The transform walks `unicode_segmentation::UnicodeSegmentation::graphemes`, never scalar `char` or byte offsets. It preserves line and span order, splits spans only at grapheme boundaries, resolves each character's effective foreground from span style, line style, and a caller-provided semantic fallback, and changes only `Style::fg`. Empty/structural line boundaries consume no rate; they become part of the visible document when the following textual content becomes visible. Printable spaces do consume a reveal step.

The track keeps a flattened rendered-grapheme signature only while that transcript block or selected Preview is active. Reconciliation compares grapheme text but excludes foreground styles and layout-only line boundaries, so theme/background changes and width reflow preserve progress. This active-only duplication is preferable to putting width/theme-dependent Ratatui lines into the semantic model or cache.

Alternative: progressively slice raw Markdown and rerun the parser. Rejected because parser markers would leak or reinterpret incomplete constructs, style could oscillate as closing syntax arrives, and raw syntax would consume the visible character budget.

Alternative: store a fade color directly in transcript/Preview spans. Rejected because it would contaminate semantic content and make theme changes, resize, cache reuse, and copy behavior stateful.

### 2. Define the profile as foreground contribution, with virtual drain steps

For background channel `b`, resolved semantic foreground channel `f`, and profile weight `w`, calculate `round(b + (f - b) * w)` independently in sRGB byte space. Thus `0` is the configured background and `1` is the original foreground. Values in the static profile are asserted or clamped to `[0, 1]` at the helper boundary.

After reveal batch `a`, every character in that batch uses `weights[0]`, every character in batch `a - 1` uses `weights[1]`, and characters whose batch age reaches `weights.len()` use their original foreground. A finite document then receives at most N fade-only batch steps after its final grapheme, moving the virtual reveal head forward without exposing content, so the final color groups do not remain permanently dark. A live assistant stream with an empty queue does not drain while more chunks can still arrive; it starts the bounded drain when its `streaming` flag becomes false. Preview content is finite as soon as it becomes Ready.

Alternative: restore the final suffix immediately. Rejected because it introduces a visible end-of-document color pop.

Alternative: run a continuous time-based alpha tween for every character. Rejected because the requested contract is position/step based and a continuous tween would require a faster periodic clock and more patches.

### 3. Batch high rates by visible frame without catch-up

A newly activated lane may reveal its first available grapheme immediately. Thereafter each due callback advances one visible batch, or one fade-drain position, per active lane. The batch contains `ceil(rate × 16ms)` graphemes and all graphemes in that batch share one fade-profile color; its next due time is `actual_tick_time + batch_size / chars_per_second`, which is never less than 16ms. Elapsed intervals are not converted into multiple batches.

The supported setting range is 0–1024 characters/s. Zero disables pacing for that lane and immediately exposes its complete semantic content. Positive values preserve the requested long-term rate while keeping terminal work at one visible batch per frame and avoiding delayed catch-up bursts.

Transcript and Preview are independent lanes: an active transcript can advance at 16/s while an active Preview advances at 32/s. Within the transcript, tracks are keyed by `DisplayId`; ordinary DSH turn serialization normally leaves one active reply, but the state shape does not assume that only one can exist.

### 4. Start transcript tracks only for new live assistant Markdown

The live assistant projection/application path starts or extends a transcript reveal track for normal assistant `TranscriptFormat::Markdown` blocks. Snapshot replay and history prepend never create tracks, so existing conversations render immediately. A finalized block can continue revealing queued text even after `streaming` becomes false; semantic settlement and copy data are not delayed.

The full `TranscriptBlock.content`, Markdown layout registry, and copy units remain authoritative. Rendering first materializes complete Markdown semantics, then applies the track's visible grapheme boundary. Reading View continues using complete provenance rather than the animated prefix. Resize rematerializes complete width-aware lines and reapplies the same logical grapheme count.

Reveal steps mark a dedicated earliest dirty transcript index. Generalize the existing tail-splice path into a suffix splice from that message's cached start through the end. This handles line-count growth without falling into a full transcript rebuild and remains safe if a later activity has been appended after an assistant block whose visual queue is still draining. Messages before the earliest changed track are not rerendered. Structural events retain their existing full invalidation rules.

Alternative: mark the message through `dirty_messages`. Rejected because a newly revealed grapheme can add or remove Markdown/wrapped rows; the current line-count-stable patch would repeatedly fall back to full rebuilds.

Alternative: force an old reveal to complete whenever another item is appended. Rejected because it creates exactly the visual burst the pacing feature is intended to prevent.

### 5. Apply Preview reveal after semantic content materialization

`PreviewPaneState` owns a selected-target reveal track alongside its semantic `PreviewState` and cache. `content_lines` continues to render every `PreviewContent` variant into fully semantic logical lines. The shared transform then clips/fades those lines before wrapping, scrolling, vertical centering, and visible-row materialization.

A Preview identity change starts a new track even when semantic content came from cache; this makes each newly shown Preview progressive and retains the existing identity-change scroll reset. A same-identity revision reconciles the longest unchanged rendered grapheme prefix. Already revealed common-prefix content stays visible, changed or appended content queues from the divergence, and the existing same-target scroll is retained. Theme-only style changes do not count as content divergence.

Ready content covered by this path includes `Link`, `Diff`, `Lines`, `SearchResult`, `Command`, `Path`, `Markdown`, `Reasoning`, `MutedMarkdown`, `PlainText`, `Tool` (including normalized terminal output), and `Hunks`. `Empty`, `Loading`, and `Error` remain immediate UI states rather than semantic Preview documents.

The Preview semantic cache continues storing complete `PreviewContent`; reveal progress never becomes part of cache keys or resolver results. A Preview reveal tick marks only Preview/frame dirtiness and cannot invalidate `TranscriptRenderCache`.

### 6. Use validated transparent config value types while preserving TOML primitives

Add the following embedded defaults and persisted keys:

```toml
background_color = "#000000"
message_chars_per_second = 120
preview_chars_per_second = 300
```

Use transparent serde value types (for example `HexRgb` and `RevealRate`) so TOML remains a string plus integers while direct `Config` deserialization validates `#RRGGBB` and `0..=1024`. A zero reveal rate disables pacing and exposes complete content immediately. These types are fields of the canonical `Config`; they are value validation, not a second persisted schema. Missing fields still inherit from `assets/default_config.toml`, and invalid known values follow the existing whole-config safe fallback/diagnostic path. The hex value is serialized in normalized lowercase form.

Add three ordinary `SettingDef` Input rows. Their `apply` functions update only after successful parse, so invalid edits leave the previous value untouched; confirmed valid edits use the existing save/reload-derived-state flow. The background is a fade reference, not a replacement for theme surface `bg`. `plain_color` keeps paced clipping but skips interpolation.

Alternative: retain unchecked `String`/integer fields and silently clamp in the renderer. Rejected because persisted invalid values would make Settings disagree with runtime behavior and spread validation across call sites.

### 7. Replace the single-cadence animation loop with earliest-deadline composition

The runner currently advances all animation through one `animation_deadline` based on `spinner_frame_ms`. Introduce independent next-due values for spinner/settle, transcript reveal, and Preview reveal, and wait on their minimum. A due tick advances only clocks whose deadline has expired, returns transcript/Preview animation dirtiness, and computes the next minimum. The current 16ms safe animation floor remains.

Frontend reveal ownership and deadline calculation live in `e-tui`; `e-dsh::AppState`/runner delegates to it while retaining any transitional DSH-side spinner sidecars. Rename or split `animation_active`/`tick_spinners` so an idle client has no deadline once spinners, settle transitions, queued graphemes, and fade drains are all complete. Inbound chunk handling only extends target state and requests an immediate content/reveal check; it does not directly expose all received text.

This preserves the existing frame scheduler's coalescing: transcript and Preview may both step in one due turn, but each lane advances only once and one atomic Ratatui frame paints the combined result.

## Risks / Trade-offs

- **[Markdown revisions change earlier rendered text]** A streaming construct can reinterpret text before the append point. → Reconcile rendered grapheme signatures, preserve only the actual common prefix, and queue from the first divergence rather than assuming raw-source append.
- **[Reveal work becomes quadratic on very long replies]** Rebuilding signatures or scanning from the beginning on every single-character tick could dominate. → Build grapheme boundaries once per target revision, keep active-only indices, and make ticks update counters/dirty ranges rather than reparse source; continue using existing Markdown layout caching.
- **[Suffix splice rerenders later items]** A reveal track that is no longer the final item can require rebuilding its following cached suffix. → Splice from the earliest active reveal only, keep prior messages untouched, and instrument/tests assert no full rebuild; normal serialized turns keep this suffix short.
- **[Styled grapheme crosses span boundaries]** Combining marks could theoretically be split between parser spans. → Coalesce adjacent textual pieces for grapheme segmentation while retaining the effective style of the grapheme's owning base span, and add combining/ZWJ regression tests.
- **[Configured background differs from a local card/code background]** The requested global reference may not visually match every semantic surface. → Treat it explicitly as a user-controlled interpolation origin and never overwrite `Style::bg`; future work can add per-surface origins without changing the profile API.
- **[Fast Preview animation increases frame activity]** High configured rates add graphemes to each visible frame batch. → Patch only Preview state, advance no more than one batch per due turn, stop after the N-step drain, and retain the 16ms global frame floor.
- **[Invalid manual config causes fallback]** Strict transparent value deserialization rejects malformed hex/rates. → Use the existing clear config diagnostic and complete embedded-default fallback; Settings prevents creating invalid values.

## Migration Plan

1. Add deterministic unit tests for reveal profile math, grapheme boundaries, prefix reconciliation, pacing, delayed deadlines, and final drain.
2. Add validated config value types, embedded defaults, and Settings rows without selecting reveal behavior yet.
3. Add transcript reveal sidecars and live/replay eligibility, then integrate the suffix-splice cache path.
4. Add selected Preview reveal state and route every Ready `PreviewContent` renderer through the shared styled-line transform.
5. Compose independent reveal and spinner deadlines in the event-driven runner.
6. Add TestBackend regressions for assistant Markdown, prompt injection Preview, structured command Preview, diff/ANSI style preservation, resize, theme/background changes, cache work, and completion.
7. Update `docs/client.md`, `docs/design.md`, and the relevant AGENTS architecture summary. Keep README concise unless implementation changes the user-facing quick reference beyond Settings values.
8. Run scoped Rust tests for config/settings/reveal/transcript/Preview/scheduler, then `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets` because the change crosses frontend and runner architecture.

Rollback is a source revert. Old user configs remain valid because missing keys inherit defaults; configs saved after the change contain only primitive TOML values and can have the three unknown keys ignored by older overlay loaders.

## Open Questions

- Whether future themes should provide per-surface fade origins. The initial implementation follows the requested single configurable `background_color` for every surface.
