## Why

The current TUI has three related presentation defects reported in GitHub issues #5, #7, and #8: long activity runs overwhelm the normal transcript, tool Preview output can displace its identifying call details, and paced Markdown code-block rendering visibly redraws its first rows. These behaviors make live work harder to follow even though the final semantic content is correct.

## What Changes

- After assistant Markdown output or an interruption/error closes an activity phase, collapse each completed run longer than six one-row activity/tool-call rows in normal mode to the first three rows, a `... (N lines)` summary, and the last three rows; keep live trailing runs and Reading View complete. Rich tool calls with informational detail do not trigger folding.
- Keep a tool Preview's wrapped identifying section visible at the top after long output reaches it, while the output uses single-row clipping rather than wrapping.
- Change Preview reveal so newly encountered non-reasoning content appears as one block/fade unit; retain row-paced reveal only for reasoning content.
- Make replayed, resumed, cached, and Reading-selected Preview content fade in as one complete page rather than replaying first-appearance pacing.
- Stabilize paced Markdown code-block layout so generated fill and clipping cannot make the first rows oscillate during streaming.
- Keep pane-separator glyph backgrounds transparent when their theme role omits an explicit background instead of substituting the base-surface color.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `event-display-surfaces`: normal-mode activity runs gain bounded middle-row folding while semantic Reading content remains complete.
- `structured-tool-preview`: tool Preview layout gains a sticky wrapped identity section and non-wrapping output rows.
- `paced-text-reveal`: Preview admission distinguishes first-live reasoning, first-live non-reasoning blocks, and historical or revisited pages, and transcript code-block reveal must remain geometrically stable.
- `unified-preview-pane`: separator bar and drag-guide roles retain terminal transparency when no background is configured.

## Impact

The change is confined to provider-neutral presentation, layout, reveal state, and focused tests under `crates/e-tui`. It does not change DSH or Pi protocols, adapter DTOs, persisted configuration, keybindings, dependencies, or complete-source copy semantics.
