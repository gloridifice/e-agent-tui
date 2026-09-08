# semantic-reading-view Specification

## Purpose
TBD - created by archiving change refactor-client-architecture-and-reading-view. Update Purpose after archive.
## Requirements
### Requirement: Semantic Reading Document
The client SHALL derive a width-independent Reading Document of stable Blocks and Items from the single transcript/provenance path. Each Block SHALL have a stable identity, semantic kind, complete copy payload, optional specialized Preview reference, and zero or more Items; each Item SHALL belong to exactly one Block.

#### Scenario: Terminal width changes
- **WHEN** a paragraph, code block, list row, Mermaid diagram, tool activity, or visible reasoning segment rewraps after resize
- **THEN** its Block identity and copy payload remain unchanged while only width-dependent geometry is rebuilt

#### Scenario: Tool lifecycle settles
- **WHEN** a running tool activity becomes successful or failed without changing semantic ownership
- **THEN** the Block retains its identity and refreshes its annotations and Preview

#### Scenario: Reasoning is hidden
- **WHEN** presentation policy does not render a reasoning segment
- **THEN** that segment does not create an invisible eligible Block

### Requirement: Shared width-dependent Reading Layout
Reading Layout SHALL derive Block row ranges, gutter rails, and all wrapped Item fragments from the same width-aware transcript representation used by rendering and copy provenance. It MUST NOT implement a second wrapping algorithm or identify semantic targets by rendered row number.

#### Scenario: Link wraps across rows
- **WHEN** one link Item materializes into multiple terminal fragments
- **THEN** spatial navigation treats the fragments as one Item identity

#### Scenario: History is prepended
- **WHEN** older transcript content is inserted above the current document
- **THEN** existing Block identities survive and their derived row geometry shifts consistently with the anchored viewport

### Requirement: Reading View entry and cursor invariants
The configurable Reading View binding SHALL default to osmain-r and enter Reading View only when an eligible Block exists, preserve the complete composer draft state, select the eligible Block nearest the viewport center, clear the Item cursor, and switch Preview to cursor-following policy. Normal mode SHALL have no Reading cursor; Reading View SHALL have exactly one Block cursor and at most one Item cursor belonging to that Block.

#### Scenario: Enter with visible Blocks
- **WHEN** the user presses the selected Reading View binding and eligible Blocks exist
- **THEN** the visually nearest Block to viewport center becomes the sole Block cursor and the composer draft remains untouched

#### Scenario: Enter with no Blocks
- **WHEN** the user presses the selected Reading View binding with no eligible Block
- **THEN** normal mode remains active and a short bounded notice is shown

#### Scenario: Resize while reading
- **WHEN** resize rebuilds Reading Layout
- **THEN** the current semantic cursors remain valid and a visible anchor is restored from their identities

### Requirement: Block navigation and semantic copy
In Block mode, configurable move_down and move_up SHALL default to j/Down and k/Up and move to the next or previous eligible Block. enter_items SHALL default to l/Right, copy_block to y, and exit to Esc/q. Copying MUST carry the complete Block source independent of clipping, wrapping, folding, and Preview rendering.

#### Scenario: Copy an atomic Block
- **WHEN** the current Block is code, table-backed custom content, or Mermaid and the user invokes copy_block
- **THEN** the clipboard action carries the complete original atomic source rather than rendered terminal characters

#### Scenario: Navigate past a boundary
- **WHEN** the cursor is on the first or last eligible Block and movement requests a nonexistent predecessor or successor
- **THEN** the cursor remains on the current Block

### Requirement: Spatial Item navigation
Item mode SHALL maintain both cursors and rank directional candidates by primary-axis distance, secondary-axis distance, then visual/document order. Configurable up and down SHALL cross to adjacent Blocks using the prior horizontal position; left at its boundary SHALL return to Block mode; right SHALL search later Items in visual order. back_to_blocks SHALL default to Backspace and return to Block mode without changing the Block cursor; inherited exit SHALL exit the complete Reading View.

#### Scenario: Move down across a Block boundary
- **WHEN** no Item exists below in the current Block and the next Block has Items
- **THEN** the Block cursor moves to that Block and Item cursor selects the Item nearest the retained horizontal position

#### Scenario: Adjacent Block has no Items
- **WHEN** vertical movement crosses to an eligible Block without Items
- **THEN** the Block cursor moves there and Item mode ends

#### Scenario: Leave Item mode at the left boundary
- **WHEN** no Item exists left of the current Item and the user invokes move_left
- **THEN** the Item cursor clears while the Block cursor remains unchanged

#### Scenario: Copy while an Item is selected
- **WHEN** the user invokes copy_block in Item mode
- **THEN** the complete owning Block is copied rather than only the Item

#### Scenario: Exit while an Item is selected
- **WHEN** the user presses the default Esc or q binding in Item mode
- **THEN** Reading View exits and restores the preserved composer

### Requirement: Cursor visibility and styling
Cursor movement SHALL keep the current Block visible using top-third and bottom-third page thresholds. The current Block SHALL use the Night base background and a Bark rail drawn in the existing outer gutter without changing content x-position or wrapping. The current Item SHALL receive a local fragment highlight, while spans with explicit local backgrounds retain them.

#### Scenario: Block enters the lower threshold
- **WHEN** navigation places the current Block in the bottom third of the transcript viewport
- **THEN** the viewport advances by one clamped page while preserving Block identity

#### Scenario: Draw the Block rail
- **WHEN** a Block is current in Reading View
- **THEN** its Bark rail replaces an existing gutter column and its content geometry matches the same width without the rail

### Requirement: Centralized and safe input routing
Reading View SHALL be handled by the central input router after higher-priority help, blocking Input Page, approval, and question interactions. Events MUST NOT be broadcast to multiple Regions. Exiting Reading View SHALL restore the composer buffer, cursor, multiline state, and completion state that existed on entry.

#### Scenario: Blocking question is active
- **WHEN** a question Input Page owns input and the user presses a Reading View key
- **THEN** the question handles or rejects the key and Reading View does not activate

#### Scenario: Exit Reading View
- **WHEN** the user exits Block mode with `Esc`
- **THEN** ordinary composer input resumes with its preserved draft state

### Requirement: Copy Mode replacement gate
The removed row-oriented Copy Mode and Ctrl-B handling SHALL remain absent. Supported-terminal input gates SHALL cover configured Reading entry, Block/Item navigation, resize, history, source copy, and bracketed paste. Reading entry SHALL default to osmain-r, while paste SHALL default to osmain-v; terminal-intercepted chords SHALL be documented as requiring terminal configuration or remapping.

#### Scenario: Compatibility gate succeeds
- **WHEN** a supported terminal delivers the selected Reading binding and bracketed paste
- **THEN** Reading activates only on the configured binding and paste remains an independent event

#### Scenario: Compatibility gate fails
- **WHEN** a terminal intercepts osmain-r before application delivery
- **THEN** the user can configure an alternate Reading chord without changing paste behavior

### Requirement: Fast Reading cursor movement
Reading move_up_fast and move_down_fast SHALL default to PageUp and PageDown, be inherited by Item mode, and execute up to 15 ordinary semantic up/down cursor steps. Each step SHALL use the current Block/Item mode, including transitions to Block mode when a destination has no Items. Movement SHALL stop at the document boundary and keep the selected target visible using existing cursor-following behavior. This SHALL NOT mean scrolling 15 display rows or one viewport page. Global page-up/page-down actions SHALL be inactive while Reading owns keyboard input, including when fast movement is disabled or remapped.

#### Scenario: Fast movement matches ordinary navigation
- **WHEN** PageDown is pressed in Reading
- **THEN** the final Block cursor, Item cursor and viewport match 15 ordinary down actions, including any mode transitions

#### Scenario: Disabled fast movement
- **WHEN** move_up_fast is nop and PageUp is pressed while Reading owns input
- **THEN** neither the cursor nor the viewport moves via a global paging fallback

#### Scenario: Ordinary input retains paging
- **WHEN** PageUp is pressed outside Reading with default mappings
- **THEN** ordinary transcript page scrolling remains unchanged

