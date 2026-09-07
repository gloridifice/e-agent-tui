## ADDED Requirements

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

## MODIFIED Requirements

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

### Requirement: Copy Mode replacement gate
The removed row-oriented Copy Mode and Ctrl-B handling SHALL remain absent. Supported-terminal input gates SHALL cover configured Reading entry, Block/Item navigation, resize, history, source copy, and bracketed paste. Reading entry SHALL default to osmain-r, while paste SHALL default to osmain-v; terminal-intercepted chords SHALL be documented as requiring terminal configuration or remapping.

#### Scenario: Compatibility gate succeeds
- **WHEN** a supported terminal delivers the selected Reading binding and bracketed paste
- **THEN** Reading activates only on the configured binding and paste remains an independent event

#### Scenario: Terminal intercepts a shortcut
- **WHEN** a terminal intercepts osmain-r before application delivery
- **THEN** the user can configure an alternate Reading chord without changing paste behavior
