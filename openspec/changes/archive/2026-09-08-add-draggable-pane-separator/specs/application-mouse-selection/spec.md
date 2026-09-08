## MODIFIED Requirements

### Requirement: Captured mouse selection coexists with application scrolling
The client SHALL retain terminal mouse capture and application-owned wheel scrolling while providing application-owned primary-button text selection. A primary-button gesture that begins on the pane separator grip SHALL be captured by pane resizing before selectable-content hit testing; every other supported primary-button press, drag, and release SHALL update selection state without changing composer focus, Reading View navigation, or Preview target selection.

#### Scenario: User drags across transcript text
- **WHEN** the user presses the primary mouse button on selectable transcript text, drags to another selectable cell, and releases
- **THEN** the client highlights the visual range during the drag and copies its rendered text on release while terminal mouse capture remains enabled

#### Scenario: User drags the pane separator
- **WHEN** the user presses within the separator grip hit area, drags, and releases
- **THEN** pane resize owns the complete gesture, any prior text selection is cleared, and no selected text is highlighted or copied

#### Scenario: Separator drag leaves its original hit area
- **WHEN** a captured separator drag moves over selectable Transcript or Preview cells
- **THEN** subsequent drag and release reports continue resizing and do not transfer ownership to text selection

#### Scenario: Wheel input remains application-owned
- **WHEN** the user turns the mouse wheel before or after making a mouse selection or pane resize
- **THEN** the existing transcript scroll and history-paging behavior continues to receive the wheel event

#### Scenario: Unsupported button is used
- **WHEN** the client receives a middle-button, secondary-button, or unpressed motion event that has no assigned behavior
- **THEN** it does not start text selection or pane resize, write the clipboard, or insert raw mouse bytes into the composer
