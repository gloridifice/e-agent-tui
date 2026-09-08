## Why

History currently bypasses pane composition and covers Preview. It should occupy only the message pane, leaving split Preview visible.

## What Changes

- Render History in place of the full-height main pane using the existing responsive split; retain Preview and its separator.
- Prefer History over narrow Preview-only presentation while open, without changing the saved Preview mode.
- Preserve existing page input ownership, independent scrolling, and conversation restoration.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `full-screen-pages`: bound History presentation to the message pane rather than the terminal.
- `session-execution-history`: update command and split-screen presentation requirements.

## Impact

Shared `e-tui` Screen composition and focused rendering regression tests; user-facing description and client architecture. No adapter, persistence, key-binding, or export changes. Existing main-only and split geometry remain authoritative under `unified-preview-pane`.
