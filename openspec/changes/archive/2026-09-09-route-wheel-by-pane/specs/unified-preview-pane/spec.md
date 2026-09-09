## ADDED Requirements

### Requirement: Wheel scrolling follows the pointed pane
Wheel input SHALL preserve terminal cell coordinates and scroll only the pane containing those coordinates, by three display rows per notch. Split-pane separator cells and coordinates outside the viewport SHALL not scroll either pane. Main-only presentation SHALL scroll Main; Preview-only presentation SHALL scroll Preview. An active History page SHALL retain Main ownership and narrow-layout precedence. Wheel input during captured separator resizing SHALL not scroll hidden pane content.

#### Scenario: Pointer moves between split panes
- **WHEN** the user turns the wheel over Main and then over Preview
- **THEN** each event changes only its pointed pane, without requiring a click or changing keyboard focus or Preview target

#### Scenario: History is open beside Preview
- **WHEN** the user turns the wheel over Preview while History occupies Main
- **THEN** Preview scrolls without moving History or the hidden transcript

#### Scenario: Single pane is visible
- **WHEN** responsive layout shows only Main or only Preview
- **THEN** wheel input within the viewport scrolls only the visible pane

#### Scenario: Pointer is on separator or outside the viewport
- **WHEN** a wheel event targets the separator column or an out-of-viewport coordinate
- **THEN** neither pane scrolls

### Requirement: Preview manual scrolling has bounded independent state
Preview wheel scrolling SHALL move from its currently presented tail or manual viewport and clamp between the first row and last full viewport. Manual row zero SHALL remain distinct from automatic tail following. Scrolling back down to the tail SHALL restore automatic following, including pinned command information. Same-target updates SHALL preserve manual review; target identity replacement SHALL reset manual review. Scroll-only frames SHALL reuse styled layout and not invalidate transcript or semantic Preview caches.

#### Scenario: Scroll upward from automatic tail
- **WHEN** overflowing Preview follows the latest content and the user scrolls upward
- **THEN** Preview moves three rows upward from the tail, clamped at row zero

#### Scenario: Reach either boundary
- **WHEN** repeated wheel input reaches the top or bottom
- **THEN** no blank overscroll occurs, row zero remains reachable, and reaching the bottom restores following

#### Scenario: Content updates during manual review
- **WHEN** the selected target receives a same-identity update while Preview is manually scrolled
- **THEN** its manual anchor is retained within the available row bounds
