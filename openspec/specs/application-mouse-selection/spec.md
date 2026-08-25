# application-mouse-selection Specification

## Purpose
TBD - created by archiving change add-application-mouse-selection. Update Purpose after archive.
## Requirements
### Requirement: Captured mouse selection coexists with application scrolling
The client SHALL retain terminal mouse capture and application-owned wheel scrolling while providing application-owned primary-button text selection. Primary-button press, drag, and release SHALL update selection state without changing composer focus, Reading View navigation, or Preview target selection.

#### Scenario: User drags across transcript text
- **WHEN** the user presses the primary mouse button on selectable transcript text, drags to another selectable cell, and releases
- **THEN** the client highlights the visual range during the drag and copies its rendered text on release while terminal mouse capture remains enabled

#### Scenario: Wheel input remains application-owned
- **WHEN** the user turns the mouse wheel before or after making a mouse selection
- **THEN** the existing transcript scroll and history-paging behavior continues to receive the wheel event

#### Scenario: Unsupported button is used
- **WHEN** the client receives a middle-button, secondary-button, or unpressed motion event that has no assigned behavior
- **THEN** it does not start a text selection, write the clipboard, or insert raw mouse bytes into the composer

### Requirement: Selection targets the last committed visible surfaces
The client SHALL hit-test pointer coordinates against selectable-row metadata from the last successfully committed frame. The first version SHALL expose visible Transcript and Preview content as independent selectable surfaces, SHALL constrain a drag to the surface where it starts, and SHALL NOT treat generated terminal padding or non-selectable UI regions as text.

#### Scenario: Transcript frame is selected
- **WHEN** a press arrives over a transcript row displayed by the last successful frame
- **THEN** the selection anchor resolves to that frame's transcript logical display row and terminal cell

#### Scenario: Preview frame is selected
- **WHEN** a press and drag occur within visible Preview content
- **THEN** selection and copied text use the wrapped, scrolled, and vertically positioned Preview rows that were displayed

#### Scenario: Drag crosses a pane boundary
- **WHEN** a drag begins in the Transcript and moves into the Preview, or begins in the Preview and moves into the Transcript
- **THEN** the focus point is clamped to the visible boundary of the starting surface and copied text does not combine the two panes

#### Scenario: Press begins in a non-selectable region
- **WHEN** a primary-button press begins in the composer, an Input Page, an accessory, the status row, the title row, or blank pane padding
- **THEN** the client does not start a new selection or copy content from that region

#### Scenario: A frame fails before commit
- **WHEN** selectable geometry is computed for a frame but terminal submission fails
- **THEN** subsequent pointer input continues to target the previously committed selection frame rather than the undisplayed geometry

### Requirement: Visual extraction respects Unicode display cells
The client SHALL order forward and backward ranges consistently, SHALL snap endpoints to complete Unicode grapheme clusters, and SHALL extract rendered text by terminal display columns. It SHALL preserve intentional intermediate blank rows, exclude generated trailing fill, and join selected visual rows with newline characters.

#### Scenario: Wide and combining graphemes are selected
- **WHEN** an endpoint lands on either terminal cell of a CJK or emoji grapheme or within a combining-mark grapheme
- **THEN** the copied range contains the complete grapheme and never an invalid or partial encoding

#### Scenario: Selection is dragged backward
- **WHEN** the focus is above or to the left of the anchor at release
- **THEN** the client normalizes the endpoints and copies the same text that the equivalent forward drag would copy

#### Scenario: Selection spans multiple wrapped rows
- **WHEN** the selected visual range crosses wrapped transcript or Preview rows
- **THEN** the client slices the first and last rows by selected columns, includes complete intermediate rows, and separates the visual rows with newlines

#### Scenario: Selected row has presentation fill
- **WHEN** a card, code row, or other styled row fills unused terminal columns with background cells
- **THEN** generated trailing fill is not appended to the copied text

### Requirement: Mouse highlighting preserves semantic presentation
The client SHALL render mouse selection as a presentation-only final layer over the selected visible cells. The layer SHALL preserve explicit semantic foregrounds, backgrounds, and modifiers, SHALL cover every terminal cell occupied by a selected wide grapheme, and SHALL NOT alter transcript cache keys, reveal signatures, Reading provenance, or Preview semantic content.

#### Scenario: Selection crosses styled Markdown
- **WHEN** a range contains inline code, a code-block fill, a diff span, or another explicit background
- **THEN** the selected cells are visibly marked without replacing the underlying semantic style values

#### Scenario: Mouse selection overlaps Reading View
- **WHEN** a mouse range overlaps a Reading View block or item highlight
- **THEN** the mouse range remains visible as the final presentation layer and Reading View continues to retain its current semantic cursor and complete-source payload

#### Scenario: Opaque overlay opens over a selection
- **WHEN** help, a suggestion popup, or another opaque overlay opens while a mouse selection exists
- **THEN** the client clears the selection before compositing the overlay and does not mark overlay text as selected

#### Scenario: Selection changes during a drag
- **WHEN** multiple drag reports arrive before the next permitted interactive frame
- **THEN** selection state applies the reports in order, requests interactive rendering, and the scheduler may coalesce their paint into the next frame without invalidating transcript content

### Requirement: Mouse release uses the existing clipboard effect
A non-empty primary-button release SHALL produce the existing owned clipboard-write effect with the extracted rendered text after UI locks are released. Successful and failed writes SHALL use the existing visible clipboard result path. A click or empty range SHALL NOT write the clipboard.

#### Scenario: Visual range is released successfully
- **WHEN** a non-empty selected range is released and the clipboard port succeeds
- **THEN** the selected rendered text is written once and the client displays the copied-line notice as a small final-layer popup for the configured duration, three seconds by default, in the form `已复制 n 行：<first six graphemes>...` with the ellipsis present only when the preview is truncated

#### Scenario: User types while the copy notice is visible
- **WHEN** the copy popup is visible and the user continues editing the composer
- **THEN** the draft and cursor remain rendered normally beneath their own surface and the popup expires without requiring another input event

#### Scenario: Clipboard write fails
- **WHEN** a non-empty selected range is released and the clipboard port returns an error
- **THEN** the client displays the existing clipboard failure notice without losing terminal control or holding a UI lock during the write

#### Scenario: Primary button is clicked without a range
- **WHEN** press and release resolve to the same grapheme boundary and no text is selected
- **THEN** the client performs no clipboard write

#### Scenario: Reading View copy follows mouse selection
- **WHEN** the user later invokes Reading View copy after making a partial visual mouse selection
- **THEN** Reading View still copies the complete canonical owning Block rather than the previous visual range

### Requirement: Terminal input normalization and lifecycle cleanup are deterministic
The client SHALL normalize primary-button press, button-motion drag, release, wheel, and focus events across Crossterm and the Windows raw VT stream. Fragmented SGR reports SHALL be reassembled before routing. Focus loss and incompatible layout or session transitions SHALL cancel an active drag or clear a stale selection before it can be painted or copied.

#### Scenario: Windows SGR report is fragmented
- **WHEN** a primary press, drag, or release SGR sequence arrives across multiple stdin chunks
- **THEN** the Windows parser emits exactly one corresponding normalized mouse event after the complete sequence arrives and converts its one-based SGR coordinates to zero-based terminal cells

#### Scenario: Windows raw VT focus report arrives
- **WHEN** Windows raw input receives a complete or fragmented `CSI I` or `CSI O` focus report
- **THEN** the parser emits the corresponding normalized focus event without delivering the control bytes to the composer

#### Scenario: Terminal loses focus during a drag
- **WHEN** focus is lost after primary press and before release
- **THEN** the active drag is cancelled and a later unmatched release does not copy or leave a phantom selection

#### Scenario: Terminal is resized
- **WHEN** resize changes selectable width or pane geometry
- **THEN** selection based on the incompatible committed-frame epoch is cleared before the new geometry is used

#### Scenario: Session or Preview identity changes
- **WHEN** the session switches, a client-side new-session draft activates, history prepend shifts transcript rows, the Preview target is replaced, or an opaque overlay opens
- **THEN** any selection whose logical surface identity or composited surface is no longer stable is cleared before paint or copy

### Requirement: Selection work remains bounded and incremental
The client SHALL build selection metadata only for selectable rows materialized in the visible frame. Pointer updates and selection painting SHALL NOT flatten the complete transcript, replay events, rebuild Reading documents, invalidate semantic Preview state, or structurally invalidate the transcript cache. The feature SHALL add no fixed polling or animation ticker.

#### Scenario: Long transcript is displayed
- **WHEN** the transcript contains thousands of messages and the user starts or updates a selection
- **THEN** selection hit testing and extraction operate on the bounded committed visible-row metadata rather than scanning all transcript messages

#### Scenario: Selection-only frame is rendered
- **WHEN** only the mouse anchor or focus changes
- **THEN** transcript cache rebuild and patch counts remain unchanged and only the affected visible presentation cells need different styles

#### Scenario: Client is idle after selection
- **WHEN** a completed selection is visible and no input, content, animation, or frame deadline is pending
- **THEN** the main loop remains idle and does not wake periodically to maintain the selection

