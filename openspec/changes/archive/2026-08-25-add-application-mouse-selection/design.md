## Context

`dshe` runs in the terminal alternate screen and enables Crossterm mouse capture so wheel input can update the application-owned transcript viewport. The terminal therefore cannot perform native drag selection. The runtime currently routes only wheel events and ignores all button presses, drags, and releases; the Windows raw VT parser likewise accepts only SGR wheel reports.

Pi's fullscreen renderer resolves the same ownership conflict by retaining mouse capture and implementing selection inside the application. It routes SGR mouse reports against the last committed layout, tracks a logical selection, paints the selected cells, extracts rendered text on release, and writes that text through an injected clipboard implementation. `dshe` already has compatible foundations: an event-driven input loop, a shared width-aware transcript layout, visible-row materialization, a responsive Preview pane, interaction-owned UI state, and an owned `UiAction::WriteClipboard` effect.

The principal constraints are:

- `e-dsh` owns terminal lifecycle, raw VT parsing, runtime routing, and concrete clipboard I/O.
- `e-tui` owns interaction state, width-dependent presentation state, layout, and rendering.
- Mouse selection must not flatten the transcript, rebuild semantic copy provenance, restore a fixed ticker, or perform clipboard I/O while a UI lock is held.
- Mouse visual copying and Reading View complete-source copying have different semantics and must remain separate.
- Selection coordinates must use terminal display cells and Unicode grapheme boundaries rather than bytes or Rust character indices.

## Goals / Non-Goals

**Goals:**

- Preserve captured-mouse wheel scrolling while adding primary-button visual selection.
- Select and copy partial rendered text from the visible transcript and Preview surfaces.
- Hit-test the last successfully committed frame so pointer coordinates match what the user saw.
- Keep drag updates interaction-paced and bounded by terminal-visible rows.
- Preserve explicit foregrounds, backgrounds, modifiers, Reading View provenance, and complete-source copy payloads.
- Normalize press, drag, release, wheel, and focus-loss behavior across Crossterm and the Windows raw VT path.
- Reuse the existing clipboard effect and visible copy-result notification.

**Non-Goals:**

- Restoring terminal-native selection while mouse capture is enabled.
- Selecting the composer, Input Pages, accessories, status rows, or session-title row in the first version.
- Selecting across Transcript and Preview in one drag.
- Drag-edge auto-scroll, off-screen range selection, history loading during a drag, double-click word selection, triple-click line selection, hyperlink activation, or right-click paste.
- Reconstructing original Markdown, table, code-fence, or Mermaid source from a visual range; Reading View remains the complete-source path.
- Bridge or wire-protocol changes.

## Decisions

### 1. Keep mouse capture and implement application-owned selection

The terminal cannot simultaneously own drag gestures for native selection and report the same gestures reliably to an alternate-screen application. `dshe` will keep its current mouse-capture lifecycle so wheel scrolling remains deterministic and will consume primary-button gestures itself.

Alternatives considered:

- **Disable mouse capture:** restores native selection but loses deterministic application scrolling and wheel-triggered history paging.
- **Document a Shift-drag bypass:** terminal-dependent and not a complete product behavior.
- **Add a mouse-capture configuration toggle only:** useful as a later compatibility option, but it does not provide both capabilities together.

### 2. Separate visual selection from semantic Reading copy

Mouse selection copies the rendered characters intersecting the selected display-cell range, including visual line breaks and presentation text. Reading View `y` continues to copy the complete canonical `ReadingCopyPayload`, including source that is clipped, folded, or represented differently on screen.

This distinction avoids weakening atomic complete-source semantics for Markdown, tables, code, and Mermaid while providing familiar local text selection.

### 3. Store interaction state and committed geometry in their lifecycle owners

A new leaf module in `e-tui` will define normalized pointer gestures, selection points/ranges, visible selectable rows, and text extraction helpers.

- `InteractionModel` owns `MouseSelection`, with idle, dragging, and selected states.
- The runner owns the last committed `SelectionFrame` as an ephemeral terminal-transaction artifact; `RenderState` retains only semantic render caches and sidecars.
- `e-dsh` maps terminal events to normalized gestures and asks the frontend selection model to update state.
- Clipboard execution remains an owned `UiAction::WriteClipboard(String)` handled after locks are released.

This preserves the dependency direction `runtime adapter -> frontend interaction/layout`, with no clipboard or terminal lifecycle dependency entering `e-tui`.

### 4. Hit-test a bounded snapshot of the last committed frame

Rendering will produce a `SelectionFrame` alongside the cursor result. It contains only selectable rows that were materialized for the current visible Transcript and Preview viewports. A frame is published only after the terminal draw succeeds, so pointer input never targets geometry that was computed but not displayed.

Conceptually:

```text
SelectionFrame
  epoch
  surfaces
    Transcript -> visible SelectableRow values
    Preview    -> visible SelectableRow values

SelectableRow
  screen y
  logical display row
  selectable x range
  plain rendered text
  grapheme byte and display-cell ranges
```

Transcript rows are registered inside the existing wrapped-row materialization loop, where global display row, scroll start, screen position, and the exact wrapped `Line` are already known. Preview rows are registered after wrapping, scroll anchoring, and vertical centering are resolved. Generated full-row background fill and terminal padding do not become selectable text.

A drag remains constrained to the surface where it began. Coordinates outside that surface clamp to its visible edge. Input and chrome regions do not produce selectable rows, so a press there clears the old selection but does not start a new one.

Alternatives considered:

- **Recompute layout on every mouse event:** risks divergence from the committed frame and violates bounded-work constraints.
- **Read arbitrary text directly from the Ratatui buffer:** simple for visible cells, but loses logical surface identity and can include generated pane padding or mix side-by-side panes.
- **Store the full transcript as selectable rows:** duplicates transcript presentation and makes pointer work proportional to history size.

### 5. Use logical row plus grapheme-cell coordinates

A `SelectionPoint` identifies a surface, logical display row, display column, and endpoint affinity. Each visible row records grapheme byte ranges and terminal cell ranges using the same `unicode-segmentation` and `unicode-width` rules as wrapping.

Press and drag coordinates snap to a whole grapheme. Forward and backward drags normalize to the same ordered range. Copy extraction slices first and last rows by selected cell columns, includes complete intermediate rows, trims only generated trailing fill, preserves intentional blank rows, and joins visual rows with `\n`. Soft-wrapped rows therefore copy with visual line breaks in this version, matching Pi fullscreen's visual-selection behavior.

### 6. Paint selection as the final presentation layer

The renderer applies `Modifier::REVERSED` to the selected terminal cells as the final layer over the selectable base surfaces, before any opaque overlay is composited. It does not replace foreground or background colors. This keeps inline-code chips, diff colors, card fills, and Reading highlights authoritative while making the selected range visible. Opening help, a suggestion popup, or another opaque overlay clears any mouse selection before that overlay is rendered; selection must never invert text that is not part of its selectable surface.

Mouse highlighting is presentation-only. It does not enter transcript signatures, cache keys, semantic source, Reading provenance, or Preview reveal state.

### 7. Auto-copy on primary-button release

A non-empty release resolves text from the committed frame and returns `UiAction::WriteClipboard`. The existing effect executor and clipboard port report success or failure. Success drives a small final-layer popup that expires after the configured duration (three seconds by default) without replacing composer text or cursor state. The popup includes the copied line count plus a grapheme-safe preview of the first six copied characters, normalizes line-breaking whitespace to spaces, and adds `...` only when more content exists. A click without a drag or a range containing no text does not write the clipboard.

The selected highlight remains until a new primary press or an invalidating lifecycle transition. This gives visible feedback after copying without introducing a new keyboard binding.

### 8. Normalize Windows SGR mouse reports and cancel incomplete drags

The Windows VT parser will decode SGR primary-button press, button-motion drag, and release reports, including reports split across input chunks, and convert SGR's one-based row/column values to Crossterm's zero-based coordinates. It will also decode focus-in `CSI I` and focus-out `CSI O` reports from the raw VT stream, because Windows `ProductionTerminalEvents` reads `WindowsRawInput` rather than Crossterm's record event source. Crossterm mouse and focus events on other platforms map to the same normalized gesture type. Unsupported buttons and unpressed pointer motion are consumed or ignored without reaching the composer.

Terminal focus reporting will be enabled and restored symmetrically. Focus loss cancels an active drag so an unmatched release cannot leave a phantom selection. Resize, session switch, draft activation, history prepend, Preview target replacement, opaque-overlay activation, and incompatible selection-frame epoch changes clear or reconcile selection before it is painted or copied.

The initial change retains the existing Crossterm mouse-capture command rather than replacing terminal negotiation with custom private-mode sequences. Button-motion-only tracking can be evaluated separately if all-motion event volume becomes measurable.

### 9. Normalize raw Backspace before asynchronous event routing

Windows Terminal's measured raw-VT byte table (captured with `cargo run -p e-dsh --example input_probe`) is: Backspace → `0x7f`, **Ctrl+Backspace → `0x17` (ETB, the Unix Ctrl+W delete-word convention)**, Ctrl+H → `0x08`, Alt+Backspace → `0x1b 0x7f`. An earlier assumption that Ctrl+Backspace arrives as `0x08` was wrong and produced fixes on branches the key never reached; treat this table as the ground truth and re-measure with the probe before changing Backspace handling again.

Because the same control bytes have other legitimate meanings, the blocking reader captures Shift/Ctrl/Alt/Backspace immediately after each successful read and sends that snapshot atomically with the byte chunk; sampling `VK_BACK` after the async handoff is unreliable because the key may already be released. `0x17` with physical Backspace becomes Ctrl+Backspace, otherwise it stays Ctrl+W. `0x08` stays Ctrl+H when Ctrl is held without physical Backspace, which keeps the help binding working, and only becomes Ctrl+Backspace when a terminal does report both. `0x7f` needs the same physical Backspace evidence to gain a Ctrl modifier. Explicit Kitty CSI-u and xterm `modifyOtherKeys` sequences retain their declared modifiers on every terminal.

The snapshot cannot cover every timing, so the composer independently treats Ctrl+W as delete-word. That is the standard Unix binding, is unbound elsewhere in this client, and makes word deletion work even when the physical-key evidence is inconclusive. Parser tests inject the chunk snapshot deterministically.

### 10. Preserve event-driven and incremental performance

Each pointer event mutates only selection state and requests the already-existing interactive frame class. Drag bursts are coalesced by the frame scheduler. Building a `SelectionFrame` is proportional to visible rows and columns, not transcript length. Selection painting touches only selected visible cells and must not invalidate `TranscriptRenderCache`, `MarkdownLayoutRegistry`, Reading layout, or Preview semantic caches.

No timer is required for the first version because edge auto-scroll is out of scope. A later auto-scroll feature must contribute an explicit deadline to the scheduler rather than adding a fixed ticker.

### 11. Model selection as a reducer over an immutable committed frame

The committed selectable frame is an ephemeral terminal-transaction artifact, not durable semantic or render-cache state. The runner owns the last successfully committed `SelectionFrame`; rendering returns a candidate frame through a local render result, and the runner replaces the committed value only after terminal submission succeeds. `RenderState` therefore does not contain committed or pending selection frames.

`MouseSelection` is a pure reducer over normalized pointer gestures plus the immutable committed frame. Its update result states whether interaction state changed and optionally owns extracted text for the clipboard effect. The selection kernel owns grapheme/cell hit testing and extraction but does not paint a Ratatui buffer; a final UI selection adapter converts rendered `Line` values into selectable rows and applies reverse-video cell ranges.

Frame revision and viewport identity are the primary invalidation mechanism. A successful frame with changed selectable geometry or text advances the revision; stale selection cannot paint or copy. Pointer handling also rejects a committed frame whose viewport differs from the currently sampled terminal size, covering terminals where resize is not delivered through the raw input stream. Focus loss and a new primary press remain explicit reducer transitions; feature branches should not duplicate structural invalidation logic with scattered clears.

Clipboard completion feeds a frontend-owned generic transient notice state. The composition root executes clipboard I/O and schedules the notice deadline, but formatting, visibility, and expiry state remain in `e-tui`; `main.rs` does not own localized notice text.

## Risks / Trade-offs

- **Visual copy inserts newlines at soft wraps** -> Document the visual-copy contract and retain Reading View for canonical source copying.
- **Streaming or history changes can invalidate logical rows during a drag** -> Bind selection to a committed-frame epoch; cancel on incompatible structural changes and explicitly clear on history prepend.
- **Wide and combining graphemes can be partially addressed by a terminal column** -> Snap both endpoints through recorded grapheme cell ranges and add CJK, emoji, combining-mark, and reverse-drag tests.
- **Selection highlighting could overwrite semantic backgrounds** -> Apply reverse-video as the final modifier instead of assigning colors, with TestBackend assertions for code, diff, card, and Reading cells.
- **Mouse movement can generate high input volume** -> Ignore motion without an active primary press and rely on existing interactive-frame coalescing; benchmark before considering custom button-motion terminal modes.
- **Clipboard release races with a new frame** -> Extract only from the last committed `SelectionFrame`; reject an epoch mismatch rather than copying geometry the user did not see.
- **Selecting only Transcript and Preview is narrower than terminal-native selection** -> Keep the selectable-surface model extensible so fixed regions can opt in later without treating padding as content.

## Migration Plan

1. Add normalized mouse parsing and selection-model tests without changing visible behavior.
2. Add committed selectable-row collection and primary-drag highlighting for Transcript.
3. Connect release extraction to the existing clipboard effect and failure reporting.
4. Add Preview selection and cross-surface clamping.
5. Add lifecycle invalidation, focus cleanup, UI regression tests, and bounded-work assertions.
6. Update `docs/client.md`, `docs/design.md`, `AGENTS.md`, README key guidance if needed, and the help overlay.

The feature is additive and requires no persisted-state or protocol migration. Rollback removes the new gesture routes and selection sidecars while leaving the existing wheel path and Reading View copy behavior unchanged.

## Open Questions

None for the initial scope. Drag-edge auto-scroll, double/triple click granularity, selectable fixed UI regions, and button-motion-only terminal negotiation are explicitly deferred follow-up decisions.
