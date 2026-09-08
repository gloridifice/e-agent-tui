## MODIFIED Requirements

### Requirement: Local history command behavior
`/history` SHALL behave identically to `/history show`. `/history show` SHALL open the full-height history view in the message pane. `/history path` SHALL insert the existing current-session trace's absolute path as editable composer text without sending a prompt, appending a transcript message, or executing a provider command. The path result SHALL use the ordinary character/atomic-block-safe insertion policy and SHALL not overwrite edits made while the request was pending. A no-trace or unmaterialized-session request SHALL show a clear unavailable state without returning the old session's path or creating a fictitious trace. Unknown subcommands or extra arguments SHALL yield local usage feedback without provider forwarding.

#### Scenario: Insert a path for an audit prompt
- **WHEN** the user executes `/history path` and the current trace is available
- **THEN** its absolute path is inserted into the composer and the user must explicitly send it to reach the AI

#### Scenario: Open without arguments
- **WHEN** the user submits `/history`
- **THEN** the same history page and data request used by `/history show` are activated

### Requirement: Title-free time-scaled history presentation
The history page SHALL place a session-wide overview timeline at the beginning of the scrollable document and display the operation-color legend once immediately below that overview. The overview and legend SHALL scroll with the per-turn content rather than remain pinned; only the bottom navigation hints remain fixed. In the default turn view, it SHALL additionally display each turn's time axis immediately before that turn's operation records, with that turn's represented start at the left endpoint and end at the right. Each turn SHALL use its own explicitly labeled time range, distinguish operation kinds by consistent theme-aware colors plus a text legend, and place overlapping operations in separate lanes. The per-turn timelines SHALL scroll with their associated records rather than being replaced by one session-wide chart. It SHALL omit the prototype's title, session masthead, and summary-header section. Exact start/end, operation summary, status and duration SHALL remain readable in a chronological record list without hover. Width SHALL encode elapsed time rather than event count; operations shorter than a terminal cell SHALL use explicitly qualified markers rather than false expanded durations. Long disconnected gaps SHALL remain identifiable; any time compression SHALL be disclosed. Empty/zero-width time ranges, tiny terminals, color-disabled output, and incomplete traces SHALL remain intelligible and bounded. History paging SHALL preserve its own viewport and SHALL not alter the conversation's viewport.

#### Scenario: Inspect a short edit beside a long command
- **WHEN** the trace contains a 20ms edit and a 12s command
- **THEN** the time display preserves their relative scale or marks the subcell edit explicitly, while the list shows both exact durations

#### Scenario: Enter with split Preview active
- **WHEN** the user opens history from the normal split screen
- **THEN** the history page replaces only the message pane without an extra history title, while Preview and the separator remain visible and the hidden conversation remains restorable

#### Scenario: Browse multiple turns
- **WHEN** the history contains several turns of different lengths
- **THEN** each turn's records are preceded by its own time-scaled timeline with visible endpoint times, not a shared unlabeled scale
