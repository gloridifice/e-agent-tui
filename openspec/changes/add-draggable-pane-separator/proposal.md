## Why

The fixed responsive split cannot be adjusted to the user's terminal, content, or current task, and its absolute-column preference does not preserve the same composition across terminal sizes. A draggable separator can make the message/Preview balance user-controlled while avoiding expensive transcript and Preview rematerialization during the gesture.

## What Changes

- Add a Bark-colored separator grip between the message pane and Preview pane, with primary-button drag resizing.
- Persist the committed message-pane width as a validated percentage and calculate pane geometry from that percentage.
- Enforce a 25% minimum message-pane share.
- Collapse Preview when its split rectangle would be narrower than 19 terminal columns (one separator column, one post-separator gap, 16 usable content columns, and one right margin); dragging that grip left restores the 16-column content area and continues resizing.
- Unify pane-level horizontal margins to one column: the Main page has one column on each ordinary edge, split Preview has one post-separator gap and one right margin, and the default user-message/input padding is one column.
- Use the theme's separator semantic backgrounds for the idle grip, drag guide, and placeholder boxes; never substitute a hard-coded palette fill.
- Render only inexpensive, margin-inset Bark placeholder boxes, a full-height Bark guide, and a thicker grip while dragging; restore and reflow real pane content once on release.
- Give separator gestures priority over application-owned text selection while preserving existing mouse capture, wheel scrolling, Reading View, and narrow full-screen Preview behavior.
- **BREAKING**: replace the persisted absolute-column `main_pane_width` preference with a percentage-based message-pane preference; old absolute values are ignored and inherit the new 60% default because they cannot be migrated without a terminal width.

## Capabilities

### New Capabilities
- `draggable-pane-separator`: Defines separator geometry, drag/collapse/restore behavior, percentage persistence, and placeholder rendering during resize.

### Modified Capabilities
- `unified-preview-pane`: Replace the fixed 60%-plus-column-cap layout and 32-column Preview minimum with the committed percentage and a 19-column split-rectangle threshold that preserves 16 usable content columns.
- `application-mouse-selection`: Give a captured separator gesture priority over selectable Transcript/Preview cells and cancel it safely on focus or geometry loss.
- `declarative-config-overlay`: Replace the absolute main-pane width setting with a validated percentage value that saves and applies immediately.
- `terminal-render-performance`: Require separator drag frames to avoid transcript and Preview layout/cache work and defer real width rematerialization until release.

## Impact

- Frontend state/config/rendering: `crates/e-tui/src/{config,settings,interaction,ui}.rs`, `crates/e-tui/src/ui/screen.rs`, embedded defaults, and UI regression tests.
- Runtime routing/persistence: `crates/e-dsh/src/runtime.rs` and `crates/e-dsh/src/main.rs`.
- Documentation: `AGENTS.md`, `docs/client.md`, and `docs/design.md`.
- No bridge, WebSocket protocol, DSH host plugin, filesystem format beyond client config, or new dependency is required.
