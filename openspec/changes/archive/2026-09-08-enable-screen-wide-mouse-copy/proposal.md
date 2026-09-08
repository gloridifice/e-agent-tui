## Why

Mouse copy currently registers only Transcript and Preview rows and deliberately excludes the composer, Input Pages, accessories, status, and title. Users need a consistent way to copy any visible TUI character, without each renderer having to opt in or accidentally exposing hidden source text.

## What Changes

- Derive visual mouse-copy text from the final composited terminal cell grid, including composer text, Input Pages, status/model labels, session title/path, accessories, suggestions, notices, and visible decorative glyphs.
- **BREAKING**: Replace pane-confined selection with one screen-coordinate, row-major range that can cross UI regions and pane boundaries. A multiline range can therefore include both panes; semantic Reading copy remains the clean complete-source alternative.
- Copy exactly the displayed representation: masked credentials remain masks, collapsed paste/image blocks remain labels, ellipses remain ellipses, and covered or unrevealed content is unavailable.
- Define Unicode-safe extraction and a visual whitespace policy: retain leading/internal spacing and blank intermediate rows, but omit trailing ordinary space cells from each extracted row.
- Keep release-to-copy, existing clipboard effects/notices, and separator resize capture. A text drag crossing the separator includes its visible glyph; a press on its resize hit area still starts resizing.
- Hold the last committed visual snapshot during an active selection gesture so streaming text, spinners, and expiring notices cannot move the selected characters. Runtime reduction continues; release or cancellation restores live presentation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-mouse-selection`: Expand selection to the final screen grid; define cross-region extraction, overlay/masking behavior, held-frame lifecycle, and bounded rendering work.

## Impact

- Primary changes: `crates/e-tui/src/mouse_selection.rs`, `ui/selection.rs`, `ui.rs`, existing Transcript/Preview selection registration plumbing, and shared runtime/controller/scheduling integration.
- Both executable runners must retain and publish committed presentation snapshots only after successful terminal submission. Shared policy belongs in `e-tui`, not duplicated adapter logic.
- Clipboard I/O remains in existing adapter ports. No DSH/Pi wire, persistence, configuration, or new dependency is planned.
- Update the client architecture's selection invariant and user-facing help/README interaction guidance during implementation. Preserve the already implemented separator behavior from the completed `add-draggable-pane-separator` change, which has not yet been archived into the active spec.
- Add focused extraction, composition, gesture-lifecycle, failed-commit, scheduler, and adapter parity regressions. This proposal does not change implementation code or active specifications.
