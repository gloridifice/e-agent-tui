## MODIFIED Requirements

### Requirement: Non-configuration surfaces remain independent
The session picker, help overlay, Reading View, approval interaction, and question bar SHALL remain outside the Input Page roster. Reading View MUST NOT activate while a blocking Input Page, approval, or question owns input.

#### Scenario: Open the session picker
- **WHEN** the user invokes the existing session-picker action while no Input Page owns input
- **THEN** the existing session picker behavior remains available and is not represented as a configuration Input Page

#### Scenario: Reading View is requested from a blocking page
- **WHEN** a blocking Input Page owns input and the user presses the Reading View binding
- **THEN** the page retains input ownership and Reading View does not activate

## ADDED Requirements

### Requirement: Page and composer state survive Reading View
Entering and exiting Reading View SHALL NOT discard or mutate the ordinary composer draft, cursor, multiline state, completion state, transcript state, or inactive Input Page session data.

#### Scenario: Return to the composer
- **WHEN** the user enters Reading View with an unfinished multiline draft and later exits
- **THEN** the same draft, cursor position, multiline state, and completion state are restored
