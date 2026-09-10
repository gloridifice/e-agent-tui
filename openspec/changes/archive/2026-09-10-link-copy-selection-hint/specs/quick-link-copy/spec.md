## ADDED Requirements

### Requirement: Link selection guidance
While one-key link selection is armed, the frontend SHALL show a localized Sage-equivalent foreground hint in the existing gap between the composer and status bar. It SHALL explain that pressing the character following `~` copies the corresponding link or path and that Esc cancels. The hint SHALL disappear when selection ends and SHALL NOT change composer content or bottom-layout height.

#### Scenario: Enter and leave selection
- **WHEN** the user enters link selection with available tags
- **THEN** the gap displays the selection guidance
- **AND** copying a target or cancelling selection restores the empty gap
