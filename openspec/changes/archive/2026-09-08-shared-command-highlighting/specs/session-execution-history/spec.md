## MODIFIED Requirements

### Requirement: Semantic history theme roles
History rendering SHALL consume fixed `semantics.history` roles rather than palette names or hardcoded RGB values. Base roles SHALL be `text`, `heading`, `metadata`, `total_elapsed`, `separator`, `hint`, `progress`, and `bar_text`. The `operation` subgroup SHALL contain `model`, `read`, `edit`, `bash`, `search`, and `other`; each role's foreground SHALL define the shared operation color for list labels, legend swatches, short markers and timeline segment fills. Embedded segment text SHALL use `bar_text`; non-command summaries SHALL use `text` independently of operation color. Command summaries SHALL use the shared command-token presentation defined by `structured-tool-preview`, with executable, argument, and operator foregrounds supplied by `history.operation.bash`, `history.text`, and `history.metadata` respectively. The `duration` subgroup SHALL contain `highest`, `second`, `top_five`, `remaining`, and `unknown`; ranking policy and eligibility SHALL remain outside the theme. Total elapsed SHALL not inherit individual-call ranking styles. Results SHALL reuse `working_status` roles and capture diagnostics SHALL reuse `log` roles; a highest-duration highlight SHALL not imply a failure outcome.

The page surface SHALL explicitly reset to the terminal default background rather than inherit `surface.base.bg`; only intentional timeline segment fills SHALL introduce backgrounds. Built-in themes SHALL explicitly configure history roles. A user theme with no history group SHALL remain loadable through a deterministic fallback derived solely from that theme's existing semantic roles, never Ferra palette names or literal colors. An explicitly supplied history group SHALL use the fixed validated schema. Ferra SHALL map text/heading/total elapsed to Mist, metadata to Bark, separator/hint to Umber, progress to Blush and bar text to Night; operation and duration mappings SHALL preserve the approved presentation requirements below.

#### Scenario: Load an existing user theme
- **WHEN** a valid custom theme without `semantics.history` is loaded
- **THEN** all history roles are derived from its existing semantic styles without requiring Ferra palette entries or invalidating the theme

#### Scenario: Reject malformed explicit history roles
- **WHEN** a supplied history group includes unknown roles, missing required roles or unresolved palette references
- **THEN** normal theme validation reports the error instead of silently replacing the explicit group with defaults

#### Scenario: Keep outcome and emphasis independent
- **WHEN** the longest completed successful operation appears in history
- **THEN** its duration uses `history.duration.highest`, its result uses `working_status.success`, and its summary uses `history.text` except for command-token presentation

#### Scenario: Command summary shares Preview token rules
- **WHEN** a recorded command includes flags, quoted arguments, command chains, and redirections
- **THEN** History applies the same token distinctions as Preview using history semantic foregrounds, clips styled text to its summary column without changing metric columns, and leaves full recorded summaries and exports undecorated

### Requirement: Ranked duration emphasis and compact outcomes
History separators and bottom key hints SHALL use the theme's Umber-equivalent tone. The page surface SHALL use the terminal default background without a Night or other page-wide color fill. Model-operation labels and legend swatches SHALL use Bark-equivalent instead of Honey-equivalent, without overriding elapsed-ranking or outcome styling. Non-command operation summary text, including model request summaries, SHALL retain Mist-equivalent; command summaries SHALL use the shared command-token presentation; timestamps SHALL use Bark-equivalent. The legend SHALL identify bash while operation records and exports retain complete recorded commands. Within the session-wide measured ranking, rank one SHALL use the failure-red tone (Ferra Ember), rank two Blush-equivalent, ranks three through five Mist-equivalent, and all remaining durations Bark-equivalent. Ranking SHALL include measured failed operations and SHALL NOT change the actual result. Success and failure in the result column SHALL render as `✓` and `✗` respectively, retaining distinguishable outcome colors; cancelled operations SHALL remain distinguishable. The page SHALL NOT change the session-wide ranking of `/history copy-10`.

#### Scenario: Highlight durations without changing outcomes
- **WHEN** the ranked list contains at least six completed operations and the longest succeeded
- **THEN** its duration is red but its result remains a success checkmark, the second duration is Blush-equivalent, ranks three through five are Mist-equivalent, and later ranks are Bark-equivalent

#### Scenario: Legend and transparent page surface
- **WHEN** the page displays its operation legend and ranked records
- **THEN** model labels use Bark-equivalent while their summaries retain Mist-equivalent and ordinary page cells retain the terminal default background

#### Scenario: Scroll past the heading and legend
- **WHEN** the user scrolls beyond the initial Top 50 heading and legend
- **THEN** both leave the viewport with the list, leaving its height available for later ranked content above the fixed navigation hints

#### Scenario: Ties and unknown duration
- **WHEN** equal measured durations and an operation with unknown duration occur in a session
- **THEN** equal durations receive ranks in execution order and the unknown duration is excluded rather than displacing a measured operation
