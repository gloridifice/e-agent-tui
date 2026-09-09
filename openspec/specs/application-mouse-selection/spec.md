# application-mouse-selection Specification

## Purpose
Define application-owned terminal mouse selection, its bounded render lifecycle and clipboard delivery, and normalized Windows raw-terminal input behavior.

## Requirements

### Requirement: Captured mouse selection coexists with application scrolling
The client SHALL retain terminal mouse capture and application-owned wheel scrolling while providing application-owned primary-button text selection. A primary-button gesture beginning on the pane separator resize hit area SHALL be captured by resizing before text selection. Other primary-button gestures SHALL select visible screen cells without changing composer focus, Reading navigation, Preview target, page focus, or editor contents.

#### Scenario: User drags across transcript text
- **WHEN** the user presses on displayed transcript text, drags, and releases
- **THEN** the client highlights the visual range and copies its rendered text on release while terminal mouse capture remains enabled

#### Scenario: User drags the pane separator
- **WHEN** the primary gesture begins in the separator resize hit area and crosses selectable cells
- **THEN** the gesture remains resize-owned, displays the existing resize presentation, and never starts text capture or writes the clipboard

#### Scenario: Separator drag leaves its original hit area
- **WHEN** a captured separator drag moves over selectable Transcript or Preview cells
- **THEN** subsequent drag and release reports continue resizing and do not transfer ownership to text selection

#### Scenario: Text drag crosses the separator
- **WHEN** a text gesture begins outside the resize hit area and crosses a visible separator glyph
- **THEN** it remains text-selection-owned, clamps to the starting pane, and excludes the separator and neighboring pane

#### Scenario: Wheel input remains application-owned
- **WHEN** the user turns the wheel before, during, or after selection
- **THEN** any active text capture is cancelled before the pane under the wheel coordinates processes the event; Main retains transcript scrolling/history paging or active History-page scrolling, while Preview scrolls independently without changing Main

#### Scenario: Unsupported button is used
- **WHEN** a middle-button, secondary-button, or unpressed motion report has no assigned behavior
- **THEN** it does not start selection, write the clipboard, or insert raw mouse bytes into the composer

### Requirement: Selection targets the last committed visible surfaces
The client SHALL derive selectable characters from the final composited cell grid of the last successfully committed frame, after ordinary overlays and notices but before mouse highlighting. The selectable domain SHALL be the entire TUI viewport, including blank cells, and SHALL NOT require individual widgets to register text. A range SHALL use row-major order within the pane containing its primary press. In a split layout, Main and Preview SHALL have independent horizontal selection bounds excluding the separator; pointer motion outside the starting pane SHALL clamp to its nearest edge. Single-pane layouts SHALL use the viewport bounds. Pane geometry SHALL be part of committed selection identity. Captured text gestures SHALL continue targeting their held committed snapshot until release or cancellation.

#### Scenario: Transcript frame is selected
- **WHEN** a press arrives over a displayed transcript row
- **THEN** the anchor resolves to its committed screen cells rather than unpainted or complete source content

#### Scenario: Preview frame is selected
- **WHEN** a gesture selects visible Preview content
- **THEN** extraction uses its final wrapped, clipped, scrolled, vertically positioned, and reveal-limited cells

#### Scenario: Drag crosses a pane boundary
- **WHEN** a text drag crosses between Main and Preview
- **THEN** the range clamps to the starting pane and intermediate rows include only that pane, without copying or highlighting the neighboring pane

#### Scenario: Press begins in a non-selectable region
- **WHEN** the user drags over composer text, an Input Page, an accessory, status labels, the title, or the working-directory path
- **THEN** those displayed characters are highlighted and copied through the same screen-selection path

#### Scenario: Press begins in blank padding
- **WHEN** a primary press starts in blank viewport cells outside the resize hit area and drags into text
- **THEN** selection starts at those screen coordinates and includes the intersected displayed text

#### Scenario: An overlay or notice is selected
- **WHEN** the user starts a selection on an already displayed suggestion, help surface, or clipboard notice
- **THEN** the copied characters are the topmost displayed characters, never the text covered beneath them

#### Scenario: Displayed content differs from source
- **WHEN** the range covers a masked credential, paste/image placeholder, truncated label, or a partially revealed block
- **THEN** extraction returns only visible masks, labels, ellipses, and revealed characters without reading or expanding the hidden source

#### Scenario: A concealed buffer symbol exists
- **WHEN** a terminal cell is rendered with concealment
- **THEN** its copy representation is the visible blank rather than the concealed symbol

#### Scenario: A frame fails before commit
- **WHEN** candidate presentation and text geometry are computed but terminal submission fails
- **THEN** they are not published and subsequent selection, if execution continues, still targets the last successful snapshot

### Requirement: Visual extraction respects Unicode display cells
The client SHALL normalize forward/backward endpoints in pane-local row-major order, include complete graphemes intersected by the inclusive range, and emit each glyph once regardless of occupied cell count. A multiline range SHALL slice the first and last screen rows at the endpoints, include complete intermediate rows within the starting pane, and join rows with newline characters. Extraction SHALL preserve leading/internal spacing and intermediate blank rows but trim trailing U+0020 space cells from each extracted row. It SHALL NOT reconstruct source whitespace or logical source lines.

#### Scenario: Wide and combining graphemes are selected
- **WHEN** an endpoint intersects CJK, emoji, combining text, or a ZWJ sequence
- **THEN** extraction includes the complete displayed grapheme once, without partial encodings or extra continuation-cell spaces

#### Scenario: Selection is dragged backward
- **WHEN** focus is above or left of the anchor
- **THEN** normalization yields the same copied text as the equivalent forward range

#### Scenario: Selection spans multiple wrapped rows
- **WHEN** a range crosses visual screen rows
- **THEN** first/last rows are sliced by their endpoints, complete intermediate pane rows are included, and visual row breaks remain newlines without including adjacent-pane text

#### Scenario: Selected row has presentation fill
- **WHEN** a styled row contains trailing ordinary space cells
- **THEN** extraction omits those trailing spaces while retaining leading/internal spaces and non-breaking spaces

#### Scenario: Range includes decorative glyphs and blank rows
- **WHEN** selected rows contain rules, arrows, bullets, diff gutters, ellipses, and an empty intermediate row
- **THEN** the visible glyphs and intermediate blank row are retained in the copied text

### Requirement: Mouse highlighting preserves semantic presentation
The client SHALL render mouse selection as the final presentation-only layer over the composited selected cells, including topmost overlay cells. It SHALL preserve underlying semantic styles, visibly distinguish selection even over an existing reversed software cursor or Reading highlight, and cover every cell of selected wide graphemes. Mouse highlighting SHALL NOT enter the unselected snapshot, transcript cache keys, reveal signatures, Reading provenance, or Preview semantic content.

#### Scenario: Selection crosses styled Markdown
- **WHEN** a range includes inline code, code fill, diff spans, or explicit backgrounds
- **THEN** the cells are visibly selected without replacing their underlying semantic style values

#### Scenario: Mouse selection overlaps Reading View
- **WHEN** a mouse range overlaps Reading Block/Item highlighting
- **THEN** the visual range remains distinguishable while Reading retains its semantic cursor and complete-source payload

#### Scenario: Opaque overlay opens over a selection
- **WHEN** a foreground overlay transition changes the interaction context during selection
- **THEN** the old selection is cancelled before the overlay is displayed, and a subsequent new gesture can select the overlay's own text

#### Scenario: Selection changes during a drag
- **WHEN** multiple drag reports arrive before the next interactive frame
- **THEN** state applies reports in order and the scheduler can coalesce highlighting over the held snapshot without invalidating transcript content

#### Scenario: Selection overlaps the software cursor
- **WHEN** a selected cell already carries the editor's reverse-video cursor style
- **THEN** selection remains visually identifiable and the copied symbol is unchanged

### Requirement: Mouse release uses the existing clipboard effect
A completed primary-button drag with a non-whitespace visual payload SHALL produce exactly one existing owned clipboard-write effect after UI locks are released. Success and failure SHALL use the existing visible clipboard-result path. A stationary click, whitespace-only range, or unmatched release SHALL NOT write the clipboard. Actual pointer movement SHALL be tracked separately from grapheme snapping so a drag confined to one wide grapheme can copy it.

#### Scenario: Visual range is released successfully
- **WHEN** a non-whitespace selected range is released and the clipboard port succeeds
- **THEN** its rendered text is written once and the existing localized final-layer copied-line notice shows the first six graphemes with an ellipsis only when truncated, for the configured duration with a three-second default

#### Scenario: User types while the copy notice is visible
- **WHEN** the user continues editing with a copy notice visible
- **THEN** the draft and cursor remain rendered normally and notice expiry requires no additional input

#### Scenario: Clipboard write fails
- **WHEN** the clipboard port rejects a released range
- **THEN** the existing failure notice is displayed without losing terminal control or holding a UI lock during the write

#### Scenario: Primary button is clicked without a range
- **WHEN** press and release occur without pointer displacement
- **THEN** no clipboard write occurs

#### Scenario: A single wide grapheme is dragged
- **WHEN** the pointer moves between occupied cells of one wide grapheme and releases
- **THEN** exactly that complete grapheme is copied even though both endpoints resolve to the same glyph

#### Scenario: The selection contains only blank cells
- **WHEN** a completed range contains no non-whitespace characters
- **THEN** it does not overwrite the clipboard

#### Scenario: Reading View copy follows mouse selection
- **WHEN** Reading copy is invoked after partial visual selection
- **THEN** it still copies the complete canonical owning Block rather than the screen range or held snapshot

### Requirement: Terminal input normalization and lifecycle cleanup are deterministic
The client SHALL normalize primary press, drag, release, wheel, and focus events across Crossterm and Windows raw VT. Fragmented reports SHALL be reassembled before routing. Focus loss, terminal resize, explicit scrolling/editing/navigation, and incompatible session or foreground interaction-context changes SHALL cancel capture before stale paint or copy. Routine background content and presentation updates SHALL NOT cancel an active held-frame selection.

#### Scenario: Windows SGR report is fragmented
- **WHEN** a primary press, drag, or release SGR sequence arrives across stdin chunks
- **THEN** the parser emits exactly one normalized event after completion and converts one-based coordinates to zero-based cells

#### Scenario: Windows raw VT focus report arrives
- **WHEN** a complete or fragmented `CSI I` or `CSI O` report arrives
- **THEN** the parser emits the corresponding focus event without inserting control bytes into the composer

#### Scenario: Terminal loses focus during a drag
- **WHEN** focus is lost before release
- **THEN** capture and its held presentation are released, and a later unmatched release cannot copy or leave a phantom selection

#### Scenario: Terminal is resized
- **WHEN** terminal dimensions change during selection
- **THEN** old coordinates and held presentation are discarded before rendering or accepting selection in the new viewport

#### Scenario: Session or Preview identity changes
- **WHEN** the session switches, a new-session draft activates, or foreground page/approval/explicit Preview navigation changes interaction context
- **THEN** old capture is cancelled even if the new screen happens to contain identical strings

#### Scenario: Background updates arrive during selection
- **WHEN** streaming, automatic Preview following, a history response, a spinner deadline, or notice expiry updates live state
- **THEN** reduction continues without replacing or cancelling the held screen

#### Scenario: User edits or navigates during selection
- **WHEN** editing, paste, scrolling, or navigation input arrives during capture
- **THEN** selection is cancelled before that input follows its ordinary active-context behavior

### Requirement: Windows raw Backspace preserves word deletion
The Windows raw-input reader SHALL attach its immediate physical modifier/Backspace snapshot to each byte chunk before asynchronous event routing. Raw `0x17` received with physical Backspace held SHALL emit Ctrl+Backspace, and otherwise SHALL emit Ctrl+W. Raw `0x08` or `0x7f` received with Ctrl and physical Backspace held SHALL emit Ctrl+Backspace. Raw `0x08` received with Ctrl held but physical Backspace not held SHALL emit Ctrl+H. Raw `0x7f` without a matching physical Backspace snapshot SHALL emit ordinary Backspace. The composer SHALL treat Ctrl+W as delete-previous-word so word deletion survives an inconclusive physical snapshot. Explicit Kitty CSI-u and xterm `modifyOtherKeys` Backspace sequences SHALL preserve their encoded modifiers on every terminal.

#### Scenario: Windows Terminal sends Ctrl+Backspace as ETB
- **WHEN** the blocking reader receives raw `0x17` and immediately observes Ctrl plus physical Backspace
- **THEN** the attached snapshot survives the async handoff, the parser emits Ctrl+Backspace, and the composer deletes the preceding word

#### Scenario: Ctrl+W remains delete-word without Backspace evidence
- **WHEN** the parser receives raw `0x17` without a physical Backspace snapshot
- **THEN** it emits Ctrl+W and the composer still deletes the preceding word

#### Scenario: Terminal encodes Ctrl+Backspace as BS or DEL
- **WHEN** the blocking reader receives raw `0x08` or `0x7f` and immediately observes Ctrl plus physical Backspace
- **THEN** the parser emits Ctrl+Backspace

#### Scenario: Raw Ctrl+H remains distinguishable
- **WHEN** the reader receives raw `0x08` while Ctrl is held and physical Backspace is not held
- **THEN** the parser emits Ctrl+H and the help overlay binding keeps working

#### Scenario: Legacy raw-VT terminal sends BS
- **WHEN** the parser receives raw `0x08` without Ctrl and without a physical Backspace snapshot
- **THEN** it emits ordinary Backspace rather than inferring a modifier from a delayed asynchronous key-state sample

### Requirement: Selection work remains bounded and incremental
Selection metadata and presentation snapshots SHALL be bounded by the visible viewport, not transcript history. Pointer updates, snapshot replay, and extraction SHALL NOT flatten the transcript, replay events, rebuild Reading documents, invalidate semantic Preview state, or structurally invalidate transcript caches. Shared policy SHALL serve both executable runners. The feature SHALL introduce no polling or animation ticker.

#### Scenario: Long transcript is displayed
- **WHEN** thousands of messages exist and the user changes a selection
- **THEN** hit testing and extraction use only the bounded held screen map without scanning messages

#### Scenario: Selection-only frame is rendered
- **WHEN** only mouse selection changes during capture
- **THEN** the held buffer is replayed with highlighting without transcript/Preview materialization, rebuild, or patch work

#### Scenario: Client is idle after selection
- **WHEN** no input, content, animation, notice, or frame deadline is pending
- **THEN** maintaining a completed selection introduces no periodic wakeups

#### Scenario: Background content dirties live presentation during capture
- **WHEN** live state becomes dirty while held-frame selection frames are submitted
- **THEN** those selection frames do not discard the obligation to render live changes after capture ends

### Requirement: Active text selection holds committed presentation
An eligible primary press SHALL hold the last successfully submitted unselected screen snapshot until release or cancellation. During capture only selection presentation SHALL change on screen; transport, agent reduction, and non-presentation runtime work SHALL continue. Release SHALL extract from the held snapshot, end the hold, and request immediate live presentation. Cancellation SHALL end the hold without copying. Presentation-only deadlines SHALL neither overwrite the held screen nor produce a busy loop, and resumption SHALL preserve normal bounded reveal pacing.

#### Scenario: Streaming continues while status text is selected
- **WHEN** agent chunks and spinner updates arrive between primary press and release
- **THEN** they reduce normally while the displayed screen remains stable and release copies exactly the selected held characters

#### Scenario: A held notice expires
- **WHEN** an already displayed notice expires during a captured selection
- **THEN** its held characters remain selectable until release/cancellation, after which live rendering observes its expired state

#### Scenario: Release restores live content
- **WHEN** capture ends after deferred live changes
- **THEN** the next live frame includes current reduced state subject to ordinary reveal pacing, and a completed highlight is retained only if its screen/context is still compatible

#### Scenario: No drag reports arrive while the button is held
- **WHEN** presentation deadlines expire but no pointer or other actionable input arrives
- **THEN** held presentation does not cause repeated full rendering or an overdue-deadline busy loop

#### Scenario: Both adapters execute the same gesture
- **WHEN** equivalent scripted screen, pointer, update, and clipboard outcomes are supplied to `dshe` and `pie`
- **THEN** shared selection policy produces the same visible range, cancellation, payload, and commit behavior
