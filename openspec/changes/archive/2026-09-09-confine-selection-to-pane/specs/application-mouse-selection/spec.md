## MODIFIED Requirements

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
- **THEN** any active text capture is cancelled before the existing transcript scrolling and history-paging behavior processes the event

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

