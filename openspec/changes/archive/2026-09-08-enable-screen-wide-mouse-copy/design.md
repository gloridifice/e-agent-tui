## Context

`mouse_selection.rs` currently identifies points by `SelectionSurface::{Transcript, Preview}`, logical row, and grapheme. `ui/selection.rs::register_line` flattens individually registered `Line` values. Other widgets never register their text, and the reducer clamps all drags to the starting surface. This is a coverage restriction, not a clipboard-port problem.

`ui.rs::render_with_cursor_and_selection` paints selection before the toast. Both executable `main.rs` loops publish candidate selection geometry only after `TerminalOwner::draw` succeeds and clear selection whenever geometry/text changes. Extending that global comparison to status spinners would make almost every active conversation cancel drags.

Existing invariants remain: ephemeral committed presentation belongs to the runner, the selection reducer is Ratatui-independent, clipboard effects execute outside UI locks, and mouse interaction must not rebuild semantic transcript/Reading/Preview state. The completed but unarchived separator change also owns primary presses on the resize hit area.

## Goals / Non-Goals

**Goals:**
- Make every displayed TUI glyph eligible for visual copy, including future widgets without selection-specific registration.
- Guarantee that highlighted cells and clipboard text describe the same visible, successfully submitted frame.
- Support forward/backward and cross-region selection, Unicode graphemes, and dynamic screen content in both `dshe` and `pie`.
- Keep bounded screen-sized work, application scrolling, separator resizing, and complete-source Reading copy.

**Non-Goals:**
- Terminal emulator title bars, other windows, scrollback outside the TUI viewport, image OCR, or bitmap copying.
- Expanding truncated strings, hidden reasoning, folded paste contents, or masked secrets.
- Editor-native cut/replace selection, word/line selection, rectangular selection, drag autoscroll, or new keyboard bindings.
- Protocol changes, new clipboard implementation, persistent selection state, or a configuration switch.

## Decisions

### 1. Capture the final cell grid instead of adding selectable widgets

Normal composition becomes:

1. Render Screen, panes, regions, help, suggestions, and notices in their existing visual stacking order.
2. Capture the final **unselected** buffer as an immutable candidate presentation snapshot, with viewport, context token, and hidden-cursor/IME anchor.
3. Adapt buffer symbols into a terminal-neutral screen cell/grapheme map in `ui::selection`.
4. Apply any valid selection highlight to the output buffer only.
5. Publish the candidate snapshot only after terminal submission succeeds.

The snapshot contains only visible cells, not the model, editor buffers, semantic sources, or covered widget text. A login field yields `●`, a paste block yields its placeholder, and an opaque suggestion replaces underlying characters. The software cursor affects styles, not the copied symbol. If a renderer uses a conceal modifier, normalize that cell to its actual visible blank rather than exporting its concealed symbol.

Ratatui buffer adaptation stays in `ui::selection`; `mouse_selection` receives plain cell coordinates, grapheme strings, and occupied widths. Remove Transcript/Preview-specific registration and its plumbing once all consumers have migrated. Do not retain both a widget-text selection pipeline and a grid pipeline.

**Alternative rejected:** registering the composer, status, and every Input Page separately. It retains the whitelist problem and requires explicit occlusion and clipping reconciliation for every overlay.

### 2. Use one row-major screen range

The screen is the selection domain. Normalize endpoints by `(row, column)`; a same-row drag selects that row's inclusive endpoint range. A multiline range selects the first row from its endpoint to the right screen edge, complete intermediate screen rows, and the last row from the left edge to its endpoint. Highlight exactly that geometry. Clamp outside-screen drag reports to the viewport.

Primary press can anchor in blank padding as well as text. Track actual pointer displacement separately from snapped grapheme endpoints: a stationary click must not copy, but a drag across the two cells of one wide glyph can copy that single glyph. Ignore drag/release reports without an active capture.

Crossing Main/Preview, an Input Page, an accessory, or an existing overlay does not change gesture ownership or clamp the range. This intentionally replaces the old pane-local behavior. Multiline selection in split mode can include Preview text and the separator; users wanting only a complete source Block retain Reading copy, and a single-row drag remains precise.

**Alternative rejected:** auto-switching from pane-local to screen selection when crossing a boundary. Its selection order jumps mid-gesture and requires maintaining two extraction models. Rectangular and pane-local optional modes can be considered separately if real usage warrants them.

### 3. Specify visual whitespace and Unicode explicitly

- Read complete buffer graphemes and associate continuation cells with their owning glyph. Never emit continuation spaces as extra text after CJK/emoji.
- Normalize endpoints and include each intersected grapheme once; highlight all of its occupied cells. Do not split combining sequences or ZWJ emoji, including at style boundaries.
- Derive extraction positions from painted cell occupancy, not UTF-8 offsets or character counts. Audit the installed Ratatui wide-cell behavior, including right-edge clipping and overlay overwrite, before implementing the adapter.
- Keep leading and internal ordinary spaces, including pane gaps and code indentation. Join selected visual rows with `\n` and retain empty intermediate rows.
- Trim trailing U+0020 cells from every extracted row. Do not apply Unicode `trim`, remove non-breaking spaces, or restore source line wrapping. A whitespace-only result does not write the clipboard.
- Copy visible arrows, bullets, rules, diff gutters, and ellipses verbatim. Exact source formatting, including significant trailing spaces, belongs to Reading copy.

A final buffer cannot reliably distinguish intentional trailing spaces from generated fill. A documented trim rule is preferable to reconstructing provenance for every UI widget.

### 4. Hold presentation during a captured text-selection gesture

On an eligible press, capture the runner's last successfully submitted unselected snapshot. Until release or cancellation, draw that snapshot plus the current selection highlight rather than recomposing live widgets. Freeze from press, not the first drag report, to avoid a moving anchor between the two events.

Agent events, transport, queues, and semantic reduction continue normally. Content/reveal/notice updates cannot replace the held screen. Preserve pending live dirty work; snapshot-only frames must not consume its obligation to redraw. On release, extract from the held frame, return the owned clipboard effect, release the hold, and request live rendering immediately. Retain a completed highlight only if the newly composed screen/context is still compatible; otherwise clear it. Clipboard notices use the normal live result path.

The shared runtime presentation policy must distinguish selection-interactive redraw requests from deferred live presentation. Suspend or acknowledge presentation-only deadlines without spinning on overdue work; restore normal deadlines on release without an unbounded reveal catch-up. A pre-existing toast may remain visually held after its semantic expiry and disappears when live presentation resumes. No new polling timer is allowed.

**Alternative rejected:** cancelling whenever any grid symbol changes. It makes status, spinner, and streaming selection unreliable. Copying an old snapshot while showing new live text is also rejected because the clipboard would disagree with the visible range.

### 5. Centralize cancellation and gesture priority

- Primary press on the existing separator resize hit area wins before text selection and never starts a held text frame. Existing resize placeholder frames remain resize-owned and non-selectable.
- A text drag that started elsewhere can pass over and copy separator glyphs without starting resize. No modifier key is necessary.
- Focus loss, resize, session/new-draft switch, explicit scrolling, text/key editing, and UI navigation cancel capture before applying their ordinary behavior. Wheel routing remains application-owned; there is no drag autoscroll.
- A foreground page/approval transition or explicit Preview target/navigation change cancels if it changes the interaction context. Routine streaming, spinner updates, automatic Preview following, history result arrival, and notice expiry only update deferred live state while held.
- Use an explicit interaction-context token to detect incompatible session/page transitions even when screen strings happen to be equal; do not compare all semantic revisions or classify every streaming update as cancellation.
- New primary press replaces old selection. An unmatched release after cancellation cannot copy.

Opening an overlay cancels an old incompatible selection; starting a new selection on an already displayed overlay is supported. The final highlight covers selected topmost overlay cells, never covered text.

### 6. Keep ownership and cost bounded

The runner owns committed and candidate presentation artifacts, using shared `e-tui` presentation helpers/policy rather than duplicating selection logic in both `main.rs` loops. Held capture can reference the immutable committed snapshot; it must not clone the entire screen per mouse report. Interaction state keeps only endpoints, capture identity, and movement state. Ratatui buffers do not enter `RenderState`, Reading, or semantic caches.

Capture/map cost is O(viewport width × height), memory is bounded by a small constant number of screen-sized snapshots, hit testing uses row/cell indexing, and extraction touches only selected rows. Selection-only frames replay the held grid without invoking transcript/Preview materialization. Terminal diffing and synchronized submission remain unchanged, and commit failure must never publish a candidate snapshot.

## Risks / Trade-offs

- [Multiline copying now includes adjacent panes] → Make row-major behavior explicit in help and acceptance checks; keep Reading complete-source copy unchanged. This is the main user-facing trade-off to confirm.
- [The display appears paused while holding the button] → Only presentation pauses; restore live rendering on release/cancellation and cover lost focus. The benefit is stable copying even from changing status text.
- [Deadline handling loses live redraws or busy-loops] → Test pending dirty work, expired toasts, reveal clocks, release, and cancellation with the scripted clock/ports.
- [Wide-cell buffers contain continuation or overwrite artifacts] → Test through the real composition path, not only hand-built strings, with CJK, combining text, ZWJ emoji, clipping, and overlays.
- [Trailing spaces lose source fidelity] → Explicit visual trim policy; Reading remains the source-preserving operation.
- [Separator glyph is not a text-selection start target] → Preserve resize UX; it remains copyable by a text drag starting in neighboring cells.
- [Completed changes have overlapping spec deltas] → Preserve separator capture when reconciling/archive ordering; do not reintroduce the older active-spec behavior.

## Migration Plan

1. Implement and test the screen map/reducer and presentation hold policy behind the existing frontend entry point.
2. Switch final composition and both runner commit paths together; remove old per-pane registration.
3. Update client architecture and localized help/README guidance, then run scoped and cross-cutting validation.
4. Ship as an interaction behavior change with no data migration. Rollback restores the prior selection/render integration; clipboard and persisted data are unaffected.

## Open Questions

No implementation-blocking unknowns remain in this proposal. Before implementation, confirm the proposed product defaults: global row-major cross-pane selection and a temporarily held display while the primary button is captured. These are recommendations, not existing behavior.
