## ADDED Requirements

### Requirement: Normal-mode activity runs are bounded at completion boundaries
The client SHALL keep a trailing live activity phase expanded in normal mode. When assistant Markdown output or an interruption/error outcome closes that phase, the client SHALL fold each completed consecutive run of more than six collapsible one-row activity messages to the first three messages, one localized `... (N lines)` summary for the omitted middle messages, and the final three messages. Activity messages with informational detail SHALL NOT themselves trigger folding. The fold SHALL be presentation-only: Reading View, semantic storage, copy provenance, activity settlement, and replay reconstruction MUST retain every original message.

#### Scenario: Seven activities remain live until Markdown output
- **WHEN** normal mode contains a trailing run of seven collapsible one-row activities and no assistant Markdown or interruption/error follows it
- **THEN** every activity remains visible and no fold summary is inserted
- **AND WHEN** an assistant Markdown block then follows the run
- **THEN** the run renders its first three activities, a summary reporting one omitted line, and its final three activities

#### Scenario: Informational tool activity does not trigger folding
- **WHEN** a run longer than six is followed by a tool activity that includes visible informational detail
- **THEN** the earlier run remains expanded
- **AND WHEN** assistant Markdown or an interruption/error later closes the activity phase
- **THEN** the eligible one-row run is folded while the informational detail remains visible in its original position

#### Scenario: Interruption or network error closes an activity run
- **WHEN** a run longer than six is followed by a user-interruption outcome or an error block such as a network failure
- **THEN** the completed run is folded before the outcome and the outcome remains visible

#### Scenario: Reading View opens a folded run
- **WHEN** the user enters Reading View while normal mode has a folded activity run
- **THEN** every original activity message is laid out in order and no synthetic fold summary replaces semantic content

#### Scenario: Completed activity run remains within the threshold
- **WHEN** assistant Markdown or an interruption/error follows a consecutive run of six or fewer collapsible activity messages
- **THEN** every message renders unchanged and no fold summary is inserted
