## Why

Streaming assistant text can visibly reflow when an incomplete trailing wrap unit grows and moves to another row. The current reveal clock also stops aging foreground colors whenever a live stream temporarily exhausts its visible queue, while Preview reveals graphemes rather than the requested display rows.

## What Changes

- Gate live assistant paint through a width-aware stable-prefix frontier so an open trailing wrap unit is held until a break, stream settlement, or bounded idle/maximum timeout releases it.
- Keep transcript reveal character-paced while separating content pacing, foreground fading, and stabilization deadlines.
- Continue foreground fade frames after the visible queue empties, including while the source stream remains open, and resume cleanly when more text arrives.
- Change Ready Preview pacing from rendered graphemes to wrapped terminal display rows, with a default maximum of 30 rows per second.
- **BREAKING**: replace the persisted `preview_chars_per_second` setting with `preview_lines_per_second`; the obsolete key is ignored by the existing known-key overlay rather than being numerically converted between incompatible units.
- Preserve complete semantic source, copy/Reading behavior, Preview cache content, incremental transcript cache work, and event-driven idle scheduling.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `paced-text-reveal`: Add stable transcript-tail admission, row-paced Preview reveal, and a fade clock that continues independently of source settlement.
- `declarative-config-overlay`: Replace the Preview character-rate key and default with a validated Preview line-rate key defaulting to 30.
- `terminal-render-performance`: Compose stabilization, reveal, and fade deadlines without restoring a fixed ticker or broad cache invalidation.

## Impact

- `crates/e-tui`: wrapping provenance/stability helpers, reveal tracks, transcript and Preview presentation, Preview state, config/defaults/settings, and UI regressions.
- `crates/e-dsh`: earliest-deadline runner integration and scheduler tests; no bridge protocol change.
- `docs/client.md`, `docs/design.md`, and `AGENTS.md`: updated reveal units, lifecycle, timeout, and performance contracts.
- Existing complete transcript/Preview semantic data and external dependencies remain unchanged.
