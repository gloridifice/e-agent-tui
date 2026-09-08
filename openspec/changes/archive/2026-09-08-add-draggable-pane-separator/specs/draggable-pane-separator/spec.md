## ADDED Requirements

### Requirement: The pane separator exposes the committed split
In normal main or split presentation, the Screen SHALL render a short vertical separator grip using the active theme's Bark-equivalent muted tone. In split presentation the grip SHALL identify the committed message/Preview boundary; when Preview is collapsed the grip SHALL remain visible inside the message pane's right margin. Full-screen Preview SHALL NOT expose a separator grip.

#### Scenario: Both panes are visible
- **WHEN** the committed percentage leaves a split Preview rectangle of at least 19 columns
- **THEN** the Screen renders non-overlapping message and Preview rectangles and a short Bark separator grip at their boundary

#### Scenario: Preview is collapsed
- **WHEN** the committed or responsive layout cannot expose the 19-column Preview rectangle required for 16 usable content columns
- **THEN** the Screen renders main-only content and keeps the short separator grip visible in the right margin

#### Scenario: Full-screen Preview is active
- **WHEN** the existing narrow Preview toggle selects full-screen Preview
- **THEN** Preview occupies the Screen and no resize grip is rendered

### Requirement: Separator dragging obeys pane limits and capture semantics
A primary-button press within the separator grip hit area SHALL start a captured pane resize gesture. While captured, drag and release coordinates SHALL update the separator even outside the initial hit area. The pending message share MUST NOT be less than 25%. A pending split Preview rectangle below 19 columns SHALL collapse Preview rather than render it narrower; the visible Preview content area SHALL be 16 columns at the threshold.

#### Scenario: User drags toward the left limit
- **WHEN** the pointer requests a message width at or below 25% of usable width
- **THEN** the pending message share remains exactly 25% and cannot be dragged smaller

#### Scenario: User crosses the Preview collapse threshold
- **WHEN** an expanded drag would leave the split Preview rectangle below 19 columns wide
- **THEN** Preview collapses, the pending message share becomes 100%, and the grip moves to the right margin

#### Scenario: User restores a collapsed Preview
- **WHEN** a drag starts on the collapsed right-margin grip and moves left
- **THEN** Preview first reappears as a 19-column rectangle with 16 usable content columns and continued leftward movement continues increasing its width

#### Scenario: Terminal cannot satisfy both limits
- **WHEN** usable width cannot simultaneously provide a 25% message pane and a 19-column Preview rectangle
- **THEN** Preview remains collapsed and the message minimum is not violated

#### Scenario: Gesture loses focus
- **WHEN** focus loss or terminal resize occurs before primary-button release
- **THEN** the pending resize is cancelled, no percentage is persisted, and a later unmatched release does not commit it

### Requirement: Separator presentation uses theme semantic backgrounds

The idle grip, full-height drag guide, and drag placeholder boxes SHALL use the active theme's `separator.bar`, `separator.line`, and `separator.placeholder` semantic styles respectively. If a custom theme omits one of these backgrounds, rendering SHALL inherit the resolved themed base surface; it SHALL NOT use a hard-coded palette color.

#### Scenario: Theme supplies separator backgrounds
- **WHEN** the active theme defines backgrounds for the separator semantic roles
- **THEN** idle grip, drag guide, and placeholder cells use those configured backgrounds

#### Scenario: Theme omits a separator background
- **WHEN** a valid custom theme leaves a separator role's background unset
- **THEN** that role inherits the theme's resolved base surface without introducing a fixed color

### Requirement: Drag frames use placeholder pane presentation
While pane resize is active, the Screen SHALL hide real message and Preview content and SHALL render the base surface, one or two margin-inset Bark placeholder boxes following the pending split, a full-height thin Bark guide, and a thicker central grip. Releasing or cancelling the gesture SHALL remove the placeholder presentation and restore normal pane content.

#### Scenario: Expanded resize is active
- **WHEN** the user holds the separator and drags within the expanded range
- **THEN** two Bark placeholder boxes resize with the pending percentage and the full-height guide remains aligned with their boundary

#### Scenario: Collapsed resize is active
- **WHEN** the pending Preview width enters the collapsed range
- **THEN** only the message placeholder remains and the full-height guide plus thick grip appear in the right margin

#### Scenario: User releases the separator
- **WHEN** a captured resize receives primary-button release
- **THEN** placeholder presentation disappears and current real content is rendered once using the committed width

### Requirement: Committed width is percentage-based and durable
The client SHALL store the message-pane width as a validated percentage from 25.00% through 100.00%, SHALL calculate pane columns from the current usable width using that percentage, and SHALL persist only a completed drag. A terminal resize that temporarily collapses Preview MUST NOT overwrite the committed percentage.

#### Scenario: Drag completes at a new split
- **WHEN** the user releases an expanded separator at a valid column
- **THEN** the client converts that column to a message-width percentage, applies it immediately, and persists the complete Config snapshot through the existing config effect

#### Scenario: Client starts at another terminal width
- **WHEN** a persisted percentage is loaded in a terminal whose width differs from the width at save time
- **THEN** the message pane is recalculated from the new usable width using the same percentage rather than the old column count

#### Scenario: Terminal temporarily narrows
- **WHEN** the stored percentage would leave a split Preview rectangle narrower than 19 columns only because terminal width decreased
- **THEN** Preview collapses without changing the stored percentage and reappears automatically after sufficient width returns

#### Scenario: User explicitly commits collapse
- **WHEN** the user releases after dragging Preview into its collapse range
- **THEN** the client persists a 100% message share until the collapsed grip is dragged left and released
