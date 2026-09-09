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
- **THEN** any active text capture is cancelled before the pane under the wheel coordinates processes the event; Main retains transcript scrolling/history paging or active History-page scrolling, while Preview scrolls independently without changing Main

#### Scenario: Unsupported button is used
- **WHEN** a middle-button, secondary-button, or unpressed motion report has no assigned behavior
- **THEN** it does not start selection, write the clipboard, or insert raw mouse bytes into the composer

