## REMOVED Requirements

### Requirement: Title-free time-scaled history presentation
**Reason**: The user requested ranking-only History rather than turn/timeline browsing; the chart and per-turn scenarios are intentionally retired.
**Migration**: Use the ranking-only message-pane presentation. Chronological detail remains available through `/history copy`.

### Requirement: Toggle a session-wide Top 50 list
**Reason**: The user requested removing the turn view, so switching between two views and preserving two offsets no longer applies.
**Migration**: `/history` directly opens Top 50 with one scroll position; the retired toggle override is ignored.

### Requirement: Per-turn duration emphasis and compact outcomes
**Reason**: Per-turn ranking and timeline-specific labels are retired with the turn browser.
**Migration**: Apply the existing duration palette and compact outcomes to the session-wide ranking; exports remain unchanged.

## ADDED Requirements

### Requirement: Ranking-only history presentation
The history page SHALL display only the session-wide Top 50 ranked operation list with an operation-color legend and fixed navigation footer. It SHALL NOT display session or per-turn timeline charts, turn groups, a turn-view selector, or a toggle hint. Loading, no eligible operations, query failure, and ready states SHALL remain distinguishable, and readable results SHALL retain incompleteness warnings even when no operations qualify. The Top 50 heading and legend SHALL scroll with the list; only bottom navigation hints remain fixed. Tiny terminals, color-disabled output, and incomplete traces SHALL remain intelligible and bounded. History scrolling SHALL preserve its own viewport without changing the conversation viewport.

#### Scenario: Enter with split Preview active
- **WHEN** the user opens history from the normal split screen
- **THEN** the ranked history page replaces only the message pane while Preview and the separator remain visible and the hidden conversation remains restorable

#### Scenario: Browse operations across turns
- **WHEN** the trace contains operations from several turns
- **THEN** history presents one elapsed-ranked list without turn headings or timeline charts

#### Scenario: Empty ranking with incomplete capture
- **WHEN** a query returns no eligible operations and a capture diagnostic
- **THEN** the page explains the empty ranking and displays the diagnostic instead of reporting a clean empty trace

### Requirement: Direct session-wide Top 50 list
Opening `/history` or `/history show` SHALL directly request and display a flat session-wide list of at most 50 calls sorted by descending measured elapsed duration, without first requesting turn pages or requiring a toggle. Execution order SHALL break ties. Eligibility SHALL match copy-10: individually measured completed tool, command and explicit model-operation spans, including measured failures/cancellations and zero durations, excluding enclosing turns/runs, idle, running and unknown-duration records. The limit SHALL count calls rather than wrapped terminal rows. With fewer eligible calls it SHALL show all of them; with none it SHALL show an explanatory empty state. Full recorded summaries and existing timing, line metrics and outcome information SHALL remain available; the list SHALL retain record identity. Ranked elapsed colors SHALL use the rank palette applied to the session-wide ordering. The view SHALL visibly identify Top 50, use one independent scroll position, and show effective scrolling/exit hints. Scrolling SHALL NOT request turn pages or trigger clipboard export. There SHALL be no History view-toggle action or default Tab binding; unrelated keys SHALL NOT mutate the retained composer. The retired `history.toggle_view` configuration entry SHALL be recognized and ignored without rejecting other valid overrides or appearing in effective help.

#### Scenario: Rank across turns on entry
- **WHEN** history contains 65 eligible calls across several turns and the user opens `/history`
- **THEN** it directly queries the full-session ranking and shows the 50 longest calls in descending duration order

#### Scenario: Retired toggle does nothing
- **WHEN** history is open and the user presses Tab with default mappings
- **THEN** no view switch, data query, export, or hidden composer edit occurs

#### Scenario: No measurable completed calls
- **WHEN** history is opened with only running or unknown-duration calls
- **THEN** an explanatory empty ranked section appears without assigning those calls zero duration

#### Scenario: Retain other custom bindings
- **WHEN** an existing mapping includes `history.toggle_view` together with valid scrolling overrides
- **THEN** the mapping loads with the scrolling overrides intact and the retired toggle has no effect or hint

### Requirement: Ranked duration emphasis and compact outcomes
History separators and bottom key hints SHALL use the theme's Umber-equivalent tone. The page surface SHALL use the terminal default background without a Night or other page-wide color fill. Model-operation labels and legend swatches SHALL use Bark-equivalent instead of Honey-equivalent, without overriding elapsed-ranking or outcome styling. Operation summary text, including model request summaries, SHALL retain Mist-equivalent; timestamps SHALL use Bark-equivalent. The legend SHALL identify bash while operation records and exports retain complete recorded commands. Within the session-wide measured ranking, rank one SHALL use the failure-red tone (Ferra Ember), rank two Blush-equivalent, ranks three through five Mist-equivalent, and all remaining durations Bark-equivalent. Ranking SHALL include measured failed operations and SHALL NOT change the actual result. Success and failure in the result column SHALL render as `✓` and `✗` respectively, retaining distinguishable outcome colors; cancelled operations SHALL remain distinguishable. The page SHALL NOT change the session-wide ranking of `/history copy-10`.

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
