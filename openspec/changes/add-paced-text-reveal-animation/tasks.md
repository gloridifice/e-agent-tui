## 1. Characterize and Build the Reveal Primitive

- [x] 1.1 Add focused characterization tests for live assistant chunk painting, transcript tail/suffix cache work, Preview identity/revision selection, semantic Preview colors, and current animation-deadline behavior.
- [x] 1.2 Add the kernel-neutral reveal module with the static `[0.217, 0.53]` profile, RGB interpolation, variable-profile support, and non-foreground style preservation.
- [x] 1.3 Implement grapheme-safe styled-line indexing, visible-prefix clipping, structural line handling, and semantic foreground resolution using the existing Unicode segmentation dependency.
- [x] 1.4 Implement deterministic `RevealTrack` pacing, immediate first step, no-catch-up deadlines, streaming/finite state, N-step final fade drain, and rendered common-prefix reconciliation.
- [x] 1.5 Add reveal unit tests for exact default/profile colors, variable N, mixed spans, whitespace/newlines, combining marks, emoji ZWJ, delayed ticks, source extension, revision divergence, and final restoration.

## 2. Add Canonical Configuration and Settings

- [x] 2.1 Add transparent validated hex-RGB and 0–1024 reveal-rate value types to the canonical `e-tui::Config` schema, serialized as the requested TOML string/integer primitives.
- [x] 2.2 Add `background_color = "#000000"`, `message_chars_per_second = 120`, and `preview_chars_per_second = 300` to `crates/e-tui/assets/default_config.toml` as the only business-default source.
- [x] 2.3 Add editable `/settings` rows for background color, transcript speed, and Preview speed, retaining the prior value on invalid confirmation and applying valid normalized values immediately.
- [x] 2.4 Extend scoped config/settings tests for old-file inheritance, valid overrides, strict invalid-value fallback, serialization, Settings validation, and immediate field updates.

## 3. Integrate Live Assistant Transcript Reveal

- [x] 3.1 Add presentation-only transcript reveal sidecars keyed by `DisplayId`, and start/extend them only from new live normal-assistant Markdown projection paths, never snapshot/history replay.
- [x] 3.2 Reconcile each active track against complete width-aware Markdown lines and apply the shared clip/fade transform without changing transcript source, copy units, Reading View, or Markdown layout ownership.
- [x] 3.3 Generalize transcript tail refresh into an earliest-dirty-message suffix splice that supports reveal-driven line-count changes without rebuilding earlier messages.
- [x] 3.4 Continue queued reveal after source settlement, drain final fade positions, retire completed sidecars, and preserve follow/non-follow scroll behavior through row growth.
- [x] 3.5 Add TestBackend and cache-work regressions for bursty chunks at 16/s, slower chunks, final backlog/drain, Markdown styles, wrapping/resize, theme/background changes, full copy/Reading source, historical immediate rendering, and non-tail suffix splicing.

## 4. Integrate Reveal Across Preview

- [x] 4.1 Add selected-target reveal state to `PreviewPaneState` without placing progress in `PreviewContent`, resolver results, or semantic cache keys.
- [x] 4.2 Start a fresh track on Preview identity changes (including cached targets), reconcile stable common prefixes on same-target revisions, and preserve existing scroll/reset and race-token semantics.
- [x] 4.3 Route every Ready `PreviewContent` variant through the shared styled-line transform after semantic line generation and before wrapping, scrolling, centering, and visible-row materialization.
- [x] 4.4 Keep Empty/Loading/Error immediate, apply plain-color pacing without RGB interpolation, and ensure Preview ticks dirty only Preview/frame state.
- [x] 4.5 Add TestBackend regressions for command/tool/terminal output, prompt-injection muted Markdown, reasoning, links, diffs, paths/lines/search, plain Markdown/text, hunks, ANSI/diff modifiers and colors, 32/s pacing, final drain, cached reselection, same-target enrichment, resize, and transcript-cache isolation.

## 5. Compose Event-Driven Animation Deadlines

- [x] 5.1 Expose next-due and due-tick operations from `e-tui` reveal state, with transcript and Preview clocks remaining independent.
- [x] 5.2 Refactor `e-dsh` spinner/settle animation helpers and runner deadline selection to wait on the earliest spinner, transcript-reveal, or Preview-reveal deadline and advance only due work.
- [x] 5.3 Request the correct animation/content frame dirtiness for reveal changes, coalesce simultaneous lane steps into one frame, and stop all animation-only wakeups when no spinner, transition, queue, or fade drain remains.
- [x] 5.4 Add deterministic runner/model tests for independent 16/s and 32/s deadlines, spinner cadence independence, the 16ms floor, delayed callback no-catch-up behavior, simultaneous due lanes, and idle shutdown.

## 6. Documentation and Validation

- [x] 6.1 Update `docs/client.md` with reveal scope, profile math, Unicode/render order, copy/history behavior, sidecar/cache ownership, settings, and deadline rules.
- [x] 6.2 Update `docs/design.md` and `AGENTS.md` with the new paced transcript/Preview animation architecture and performance constraints; keep README unchanged unless the implemented user-facing basics require a concise settings mention.
- [x] 6.3 Run the scoped reveal, config/settings, transcript cache/UI, Preview UI/state, model, and scheduler tests added or affected by this change.
- [x] 6.4 Run `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets`; verify no bridge protocol-contract change was introduced.
