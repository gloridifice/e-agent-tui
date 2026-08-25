## Why

The TUI enables terminal mouse capture so it can provide application-owned transcript scrolling, but it currently discards every mouse event except wheel input. As a result, terminal-native drag selection is unavailable and the application provides no equivalent way to select and copy an arbitrary visible text range.

## What Changes

- Add application-owned primary-button drag selection while retaining mouse capture and wheel scrolling.
- Support visible text selection in the transcript and Preview surfaces, with selection constrained to the surface where the drag begins.
- Render the selected terminal-cell range without overwriting semantic foregrounds, backgrounds, or complete-source copy provenance.
- Copy the selected rendered text to the existing clipboard effect on mouse release and reuse the existing success/failure notice path.
- Parse primary-button press, drag, and release consistently through Crossterm and the Windows raw VT input path, including fragmented SGR mouse sequences.
- Preserve complete-source semantic copying in Reading View as a separate interaction from partial visual mouse copying.
- Keep selection hit testing bounded to rows from the last committed visible frame; do not flatten or rebuild the full transcript per mouse event.

## Capabilities

### New Capabilities
- `application-mouse-selection`: Application-owned visual text selection, highlighting, extraction, clipboard delivery, input normalization, and lifecycle behavior for captured-mouse alternate-screen rendering.

### Modified Capabilities

None.

## Impact

- `crates/e-dsh`: terminal mouse event parsing/routing, Windows VT input, runtime clipboard-effect dispatch, and terminal focus cleanup.
- `crates/e-tui`: interaction-owned selection state, committed-frame selectable-row metadata, transcript/Preview hit testing, Unicode cell slicing, and final-layer selection rendering.
- UI and runtime tests: SGR parsing, forward/backward and multi-row selection, Unicode grapheme boundaries, split-pane constraints, style preservation, clipboard success/failure, and bounded cache work.
- Documentation and help text: distinguish visual mouse copying from Reading View complete-source copying.
- No wire-protocol, bridge, or external dependency changes are expected.
