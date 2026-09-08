## ADDED Requirements

### Requirement: Pane split percentage is the canonical persisted width setting
The embedded default TOML and directly deserializable `Config` schema SHALL define one validated `message_pane_percent` value as the sole persisted pane-width authority. Its default SHALL be 60%, its accepted range SHALL be 25% through 100%, and Settings plus separator release SHALL save and apply a valid value immediately. Pane columns MUST be derived from current usable terminal width and MUST NOT be persisted separately.

#### Scenario: Existing config predates percentage sizing
- **WHEN** a valid user config omits `message_pane_percent`
- **THEN** loading succeeds and inherits the embedded 60% default

#### Scenario: User file contains the obsolete absolute width
- **WHEN** a user config contains `main_pane_width`
- **THEN** the known-key overlay ignores it as obsolete and does not reinterpret it using an arbitrary terminal width

#### Scenario: User supplies a valid percentage
- **WHEN** a user config provides a value from 25% through 100%
- **THEN** that value overrides the embedded default and round-trips through canonical Config persistence

#### Scenario: User supplies an invalid percentage
- **WHEN** a known percentage value is below 25%, above 100%, non-finite, or not numeric
- **THEN** strict Config deserialization follows the existing safe diagnostic/fallback path rather than constructing an invalid layout value

#### Scenario: Settings changes the pane percentage
- **WHEN** the user confirms a valid message-pane percentage in Settings
- **THEN** the new percentage takes effect immediately and is persisted without adding a parallel settings-owned schema
