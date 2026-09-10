## MODIFIED Requirements

### Requirement: Preview manual scrolling has bounded independent state
Preview wheel scrolling SHALL move from its currently presented tail or manual viewport and clamp between the first row and last full viewport. Manual row zero SHALL remain distinct from automatic tail following. Wheel input SHALL enter manual review even at the bottom, and reaching the bottom SHALL NOT restore automatic following or pinned command information. Same-target updates SHALL preserve manual review; target identity replacement SHALL reset manual review. Scroll-only frames SHALL reuse styled layout and not invalidate transcript or semantic Preview caches.

#### Scenario: Scroll upward from automatic tail
- **WHEN** overflowing Preview follows the latest content and the user scrolls upward
- **THEN** Preview moves three rows upward from the tail, clamped at row zero

#### Scenario: Reach either boundary
- **WHEN** repeated wheel input reaches the top or bottom
- **THEN** no blank overscroll occurs, row zero remains reachable, and reaching the bottom retains the last full manual viewport without restoring pinned command information

#### Scenario: Content updates during manual review
- **WHEN** the selected target receives a same-identity update while Preview is manually scrolled, including at the previous bottom
- **THEN** its manual anchor is retained within the available row bounds rather than following new content

#### Scenario: Wheel down while already following the tail
- **WHEN** Preview is automatically following the tail and the user scrolls down
- **THEN** Preview enters manual review at the last full viewport and subsequent same-target growth does not resume following
