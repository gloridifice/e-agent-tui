## ADDED Requirements

### Requirement: Long terminal output preserves tool information
A structured tool Preview with terminal secondary output SHALL wrap its tool-name and primary-information section to the Preview content width, but SHALL render each terminal-output source row as one display row clipped at the right edge without an added ellipsis. While the complete presentation fits, the combined content SHALL retain ordinary vertical centering. Once output growth would scroll the information section beyond the top edge, the information section SHALL remain pinned at the top and the remaining viewport rows SHALL show the newest terminal-output tail.

#### Scenario: Tool Preview fits in the pane
- **WHEN** the wrapped tool information and terminal output fit within the Preview height
- **THEN** the combined presentation remains vertically centered and no sticky positioning is applied

#### Scenario: Long output reaches the top edge
- **WHEN** terminal output grows until the bottom-anchored presentation would move the tool information above the Preview
- **THEN** the complete wrapped information section remains visible at the top and output occupies only the rows below it

#### Scenario: Output line exceeds the pane width
- **WHEN** one terminal-output source row is wider than the Preview content width
- **THEN** it occupies exactly one display row, is clipped at the right edge, and receives no synthetic ellipsis

#### Scenario: Wrapped information consumes the available height
- **WHEN** the tool information section alone is at least as tall as the Preview viewport
- **THEN** the viewport prioritizes the top of the information section and renders no terminal-output row over it
