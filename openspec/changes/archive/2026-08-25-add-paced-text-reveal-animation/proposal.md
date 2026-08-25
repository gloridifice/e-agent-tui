## Why

Assistant output can arrive in uneven bursts, which makes the transcript jump and destabilizes the user's visual focus. Preview content also appears all at once, so a shared paced reveal with a short foreground fade is needed to make newly visible text consistent across both panes.

## What Changes

- Add a reusable text-fade operation that blends a configurable background reference color toward each span's semantic foreground color for the newest visible characters.
- Define the fade profile as one static variable-length weight array, initially `[0.217, 0.53]`; each newly revealed character uses the first weight, older affected characters advance through the array, and the character leaving the profile returns to its original foreground.
- Pace assistant Markdown reply text at a configurable maximum reveal speed, initially 120 characters per second, independently of bursty upstream chunk arrival.
- Make every textual Preview content kind reveal progressively, apply the same fade profile, and use an independent configurable maximum speed, initially 300 characters per second.
- Add `/settings` entries and persisted config values for the fade background/reference color (`#000000`), transcript reveal speed (`120`), and Preview reveal speed (`300`), with immediate apply and safe validation.
- Preserve complete source/copy data, Markdown semantics, Preview identity/scroll behavior, history behavior, and event-driven idle scheduling while only changing what is currently painted.

## Capabilities

### New Capabilities
- `paced-text-reveal`: Defines the reusable fade contract, character pacing, transcript and Preview coverage, Unicode/style behavior, lifecycle rules, and completion behavior.

### Modified Capabilities
- `declarative-config-overlay`: Adds the three persisted, defaulted, settings-editable reveal configuration values without introducing a parallel config schema.
- `terminal-render-performance`: Extends dirty-driven animation scheduling and incremental cache requirements to paced text reveal in the transcript and Preview.

## Impact

- `crates/e-tui`: reveal state and reusable styled-text transformation, transcript/Preview presentation, config schema/defaults, settings metadata, cache dirty tracking, and UI regression tests.
- `crates/e-dsh`: event-driven animation deadline integration and clock/tick adaptation; no new bridge message or protocol version is expected.
- `docs/client.md` and `docs/design.md`: reveal semantics, configuration, scheduling, cache behavior, and surface coverage.
- Existing user config files remain compatible because omitted fields inherit the embedded defaults. No new external dependency or filesystem access is expected.
