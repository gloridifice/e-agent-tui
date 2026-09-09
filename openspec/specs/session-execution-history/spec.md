# session-execution-history Specification

## Purpose
Provide discoverable workspace-scoped execution traces in centralized frontend storage with reliable timing and activity metrics, plus terminal inspection and human-readable audit exports without retaining tool output.

## Requirements

### Requirement: Session-local execution files
For each materialized session observed through `pie` or `dshe`, the client SHALL record execution history exclusively in `<e-config>/cache/e-pi/history/<workspace-key>/<session-key>.jsonl` or `<e-config>/cache/e-dsh/history/<workspace-key>/<session-key>.jsonl` respectively. Workspace identity SHALL follow the backend-confirmed absolute session cwd using versioned lexical normalization without Git-root promotion, symlink resolution, or blanket case folding. Workspace and session keys SHALL be stable and path-safe. File headers SHALL retain and validate provider/session identity, original cwd, and normalized workspace identity; equivalent normalized cwd spellings SHALL resolve to the same trace. Resume SHALL append to the same file with a new run identity. Native backend session storage SHALL remain unchanged. The client SHALL NOT read, migrate, delete, or modify legacy project-local execution histories or ignore files. It SHALL NOT automatically delete old traces or fall back to project-local storage when the user configuration root is unavailable.

#### Scenario: Launch from a repository subdirectory
- **WHEN** the backend confirms that the session cwd is a repository subdirectory
- **THEN** the trace is stored in that subdirectory's distinct workspace bucket under the frontend's central history root, not in the project or a Git-root bucket

#### Scenario: Resume and switch
- **WHEN** a previously recorded session is resumed and later replaced with a different session
- **THEN** its existing central file is appended for the resumed run and subsequent operations use the replacement session's confirmed identity and cwd

#### Scenario: Deferred new conversation
- **WHEN** a client-only new-conversation draft has no materialized session
- **THEN** no trace is created for it and history commands do not expose the retained previous session's trace

#### Scenario: Legacy history exists
- **WHEN** an attached session has project-local execution history but no central trace
- **THEN** recording starts a new central trace without reading or importing the legacy file, recovering its metrics, or changing project files

### Requirement: Output-free versioned execution records
The UTF-8 JSONL format SHALL declare its schema version and use ordered event identities, run identity, operation/call identity, and available turn/parent correlation. Records SHALL retain operation kind/name, command or path summary, start/end timestamps with explicit units, duration and timing source, outcome, and optional typed line metrics with truncation provenance. Starts and terminal events SHALL be independently recorded so incomplete operations remain detectable. Session attachment, detachment, and observed turn/model-operation boundaries SHALL provide execution context without storing conversational text. Capture SHALL NOT persist file bodies, replacement text, patches, tool stdout/stderr, reasoning, assistant answers, full prompts, or raw generic argument objects. Known tools SHALL use allowlisted summary fields; unknown tools SHALL retain identity without raw arguments. Known credential fields SHALL be redacted, with command text explicitly treated as potentially sensitive rather than guaranteed secret-free.

#### Scenario: Edit a file and run a command
- **WHEN** edit and bash operations finish with large outputs
- **THEN** the trace retains their path/command, status, timing, and available line metrics but contains neither replacement text nor command output

#### Scenario: Unknown extension tool
- **WHEN** an unknown tool carries arbitrary structured arguments
- **THEN** its tool/call identity and observable lifecycle are recorded without serializing those arguments

### Requirement: Truthful execution timing and coverage
Tool duration SHALL measure execution boundaries, not model argument generation or UI replay. Available valid backend execution timing SHALL be preferred; otherwise a monotonic clock at adapter event reception SHALL provide explicitly client-observed duration. Wall-clock timestamps SHALL locate events but SHALL NOT replace monotonic elapsed measurement for that fallback. Measured zero, missing timing, running, and unknown termination SHALL remain distinct. Disconnected/native-only execution SHALL NOT be represented as fully observed. Parallel operations SHALL retain separate intervals; enclosing turn/run elapsed time SHALL NOT be computed by summing overlapping operations. Model-operation spans SHALL require observable boundaries and SHALL NOT be inferred from reasoning text. Subsecond measured durations SHALL display in milliseconds.

#### Scenario: Start and end reach the same UI batch
- **WHEN** a tool's start and end are reduced before another frame is drawn
- **THEN** its duration still uses captured ingress/backend timing rather than the time between UI reductions

#### Scenario: Interrupted operation
- **WHEN** recording ends without a terminal event for a started operation
- **THEN** historical inspection marks its termination and final duration as unknown, not success or zero seconds

#### Scenario: Two tools overlap
- **WHEN** two ten-second calls execute concurrently within one ten-second interval
- **THEN** both calls retain ten-second durations and the enclosing interval is not labeled twenty seconds

### Requirement: Shared activity metrics and replay enrichment
Recorded line metrics SHALL use the same normalized values and truncation semantics as live activity rows, with absence distinct from zero and observed output counts distinct from requested read limits or file-change counts. They SHALL be captured from already available results/metadata without extra file reads or client-computed diffs. On resume, matching recorded operations SHALL enrich native replay with their saved duration and line metrics without generating duplicate calls, results, or trace records. Existing create-row suppression and file-folding display policy SHALL remain in force. Execution records SHALL remain per-operation so folding does not destroy audit detail. Missing records SHALL NOT cause invented metrics; unavailable duration SHALL not be presented as a measured zero. Trace metadata SHALL NOT substitute for missing output or diff Preview content.

#### Scenario: Restore command metrics
- **WHEN** a resumed native tool call/result matches a saved record with 24 observed output lines and 2400ms duration
- **THEN** the restored activity uses those same metrics without retaining command output in the trace

#### Scenario: Truncated read output
- **WHEN** a read requested 200 lines but only a truncated 30-line result was available to the frontend
- **THEN** any saved observed output count remains 30 with truncation qualification, not an asserted 200-line actual read

#### Scenario: Replay old native history
- **WHEN** a native historical call has no execution trace entry
- **THEN** it is not appended as a newly executed operation and no observed execution duration is fabricated

### Requirement: Explicit persistence and query failures
Trace writes and queries SHALL run outside UI state locks, preserve event order, and remain bounded without silently dropping records. A start SHALL be queued for persistence when observed rather than held until completion; accepted writes SHALL be flushed on orderly shutdown and before a successful path/export snapshot response. Power-loss durability SHALL NOT be claimed. Readers SHALL distinguish a partial final record, malformed records, unsupported versions, and identity mismatches from valid complete history; readable valid records SHALL remain inspectable with a visible incompleteness diagnostic. Competing writers SHALL not interleave or corrupt a session file; an unavailable writer, queue overflow, or I/O failure SHALL visibly mark recording incomplete/unavailable while leaving normal agent interaction usable. Queries SHALL be scoped by session/request identity and a finite record watermark. No operation SHALL silently return partial clipboard content as a complete export.

#### Scenario: Read-only cwd or competing writer
- **WHEN** the session cwd is read-only but central storage is writable and no competing writer holds the session trace
- **THEN** recording succeeds without writing into cwd; a competing session writer still causes an explicit recording-unavailable failure

#### Scenario: Unavailable central root or competing writer
- **WHEN** a trace cannot be safely opened in the central history root or the user configuration root cannot be resolved
- **THEN** the client reports recording unavailable, continues the session, and does not fall back to another location

#### Scenario: Truncated final JSONL record
- **WHEN** a crash leaves valid complete records followed by an incomplete line
- **THEN** inspection exposes the valid records with a warning and further recording does not concatenate new JSON onto the partial line

#### Scenario: Delayed result after session replacement
- **WHEN** a history query finishes after the active session changes
- **THEN** it does not update the new page, insert the old path into its composer, or trigger a stale clipboard write

### Requirement: Local history command behavior
`/history` SHALL behave identically to `/history show`. `/history show` SHALL open the full-height history view in the message pane. `/history path` SHALL insert the existing current-session trace's absolute path as editable composer text without sending a prompt, appending a transcript message, or executing a provider command. The path result SHALL use the ordinary character/atomic-block-safe insertion policy and SHALL not overwrite edits made while the request was pending. A no-trace or unmaterialized-session request SHALL show a clear unavailable state without returning the old session's path or creating a fictitious trace. Unknown subcommands or extra arguments SHALL yield local usage feedback without provider forwarding.

#### Scenario: Insert a path for an audit prompt
- **WHEN** the user executes `/history path` and the current trace is available
- **THEN** its absolute path is inserted into the composer and the user must explicitly send it to reach the AI

#### Scenario: Open without arguments
- **WHEN** the user submits `/history`
- **THEN** the same history page and data request used by `/history show` are activated

### Requirement: Human-readable history exports
`/history copy` SHALL copy a complete human-readable chronological snapshot of the current trace, including operation summaries, start/end information, outcome, duration/timing qualification, available line metrics, and completeness warnings. `/history copy-10` SHALL copy at most ten individually measured completed tool, executable-command, or explicit model-operation spans sorted by descending duration, with execution order as the stable tie-breaker. Enclosing turns/runs, idle intervals, running operations, and unknown durations SHALL be excluded from ranking; failed/cancelled operations with measured durations SHALL remain eligible. Both exports SHALL use full recorded summaries rather than screen-clipped text, omit ANSI decoration and excluded payloads, and use the existing clipboard success/error feedback. Empty input or no eligible operations SHALL produce an explanatory human-readable export rather than stale clipboard success.

#### Scenario: Rank mixed operations
- **WHEN** recorded history includes overlapping calls, a failed measured call, an enclosing turn, and an incomplete call
- **THEN** copy-10 ranks eligible individual measured calls, includes the failed call if it qualifies, and excludes the enclosing turn and incomplete call

#### Scenario: Copy while the session runs
- **WHEN** the user requests an export during active execution
- **THEN** the export ends at its request watermark, identifies still-running records as such, and does not wait indefinitely for future events

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

### Requirement: Recoverable workspace registry
Each frontend history root SHALL contain a versioned `workspaces.json` mapping workspace directory keys to normalized absolute workspace paths and readable original paths. Concurrent registration SHALL preserve unrelated mappings through bounded cross-process locking and atomic replacement. Event appends SHALL NOT rewrite this registry. Missing or malformed registries SHALL be recoverable from validated headers in the new history root only; malformed originals SHALL be preserved before replacement. Unsupported versions and identity conflicts SHALL be explicit failures rather than silently overwritten mappings. Moving a workspace SHALL create a distinct bucket rather than guessing an association with an old path.

#### Scenario: Concurrent workspace registration
- **WHEN** different clients register different workspaces in the same frontend history root
- **THEN** both mappings survive and their session records remain in separate buckets

#### Scenario: Missing or corrupt registry
- **WHEN** the registry is missing or malformed and valid central trace headers exist
- **THEN** their workspace mappings are recovered, any malformed registry is preserved, and existing session records remain intact

#### Scenario: Conflicting or unsupported metadata
- **WHEN** a registry or trace claims an incompatible version or a directory mapping inconsistent with its workspace identity
- **THEN** recording reports the conflict without overwriting that metadata or appending to the mismatched trace
