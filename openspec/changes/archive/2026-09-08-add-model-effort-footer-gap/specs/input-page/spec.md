## ADDED Requirements

### Requirement: Model and effort footer separation
The `/model` and `/effort` Input Pages SHALL reserve one blank row between the body and key-hint footer whenever the allocated page height can accommodate the shell and gap. Preferred page height SHALL include this row, and scrolling SHALL exclude it from the visible body capacity. Other Input Pages SHALL retain their existing spacing.

#### Scenario: Content-sized page
- **WHEN** a model or effort page has enough space to display its complete body
- **THEN** one blank row separates the body from the key-hint footer

#### Scenario: Height-capped page
- **WHEN** a model or effort list exceeds the available body height
- **THEN** visible options scroll above the reserved blank row without occupying it or overwriting the footer
