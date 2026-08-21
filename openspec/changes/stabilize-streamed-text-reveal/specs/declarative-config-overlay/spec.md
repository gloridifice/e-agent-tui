## MODIFIED Requirements

### Requirement: Reveal configuration is persisted through the canonical schema
The embedded default TOML and the directly deserializable `Config` schema SHALL define `background_color`, `message_chars_per_second`, and `preview_lines_per_second` exactly once as persisted values. Their defaults SHALL be `"#000000"`, `120`, and `30` respectively. Existing user files that omit them SHALL inherit these defaults through the existing known-key overlay. The obsolete `preview_chars_per_second` key SHALL be ignored as an unknown key and SHALL NOT be numerically converted to rows per second.

#### Scenario: Existing config predates reveal settings
- **WHEN** a valid user config omits all current reveal fields
- **THEN** loading succeeds with `background_color = "#000000"`, `message_chars_per_second = 120`, and `preview_lines_per_second = 30`

#### Scenario: User overrides reveal settings
- **WHEN** a user config supplies valid values for one or more current reveal fields
- **THEN** those values override the embedded defaults and all unrelated values continue to inherit or override through the existing recursive overlay

#### Scenario: User file contains the obsolete Preview character rate
- **WHEN** a user config contains `preview_chars_per_second` but not `preview_lines_per_second`
- **THEN** the obsolete key is ignored and Preview uses the default line rate of 30

### Requirement: Reveal settings are editable and validated
The `/settings` Input Page SHALL expose editable rows for the fade background/reference color, transcript maximum grapheme reveal speed, and Preview maximum display-row reveal speed. Confirmed values SHALL save and apply immediately. `background_color` SHALL accept exactly a six-digit `#RRGGBB` value, case-insensitively; both speed values SHALL accept whole units-per-second values from 0 through 1024. Zero SHALL disable pacing for its lane and expose complete content immediately. Invalid edits SHALL NOT replace the last valid configured value.

#### Scenario: User changes the fade background
- **WHEN** the user confirms `#1a2B3c` in the background-color row
- **THEN** the canonical persisted value becomes a valid normalized hex color and active faded groups are repainted against it

#### Scenario: User enters an invalid color
- **WHEN** the user confirms a value that is not exactly `#RRGGBB`
- **THEN** the settings page retains the previous valid `background_color` and does not persist the invalid value

#### Scenario: User changes transcript speed
- **WHEN** the user confirms a whole value from 0 through 1024 for the transcript reveal row
- **THEN** `message_chars_per_second` uses and persists that grapheme rate immediately

#### Scenario: User changes Preview speed
- **WHEN** the user confirms a whole value from 0 through 1024 for the Preview reveal row
- **THEN** `preview_lines_per_second` uses and persists that wrapped-row rate immediately

#### Scenario: User enters an out-of-range speed
- **WHEN** the user confirms a value above 1024, a negative value, or a non-whole value
- **THEN** the settings page retains the previous valid speed and does not persist the invalid value
