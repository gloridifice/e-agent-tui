## Purpose

Provide discoverable project-local execution traces with reliable timing and activity metrics, plus terminal inspection and human-readable audit exports without retaining tool output.

## ADDED Requirements

### Requirement: Session-local execution files
For each materialized session observed through `pie` or `dshe`, the client SHALL record execution history in `<session.cwd>/.e/e-pi/execution-history/<session-key>.jsonl` or `<session.cwd>/.e/e-dsh/execution-history/<session-key>.jsonl` respectively. The directory SHALL follow the backend-confirmed session cwd without Git-root promotion. Session keys SHALL be stable and path-safe; file headers SHALL retain and validate provider/session identity and cwd. Resume in the same cwd SHALL append to the same file with a new run identity. Native backend session storage SHALL remain unchanged. Automatic ignore setup SHALL ignore only these execution-history directories and preserve existing `.e/.gitignore` entries. The client SHALL NOT automatically delete old traces or silently relocate writes to a user directory.

#### Scenario: Launch from a repository subdirectory
- **WHEN** the backend confirms that the session cwd is a repository subdirectory
- **THEN** the trace is stored beneath that subdirectory's `.e`, not beneath the repository root

#### Scenario: Resume and switch
- **WHEN** a previously recorded session is resumed and later replaced with a different session
- **THEN** its existing file is appended for the resumed run and subsequent operations use the replacement session's confirmed identity and cwd

#### Scenario: Deferred new conversation
- **WHEN** a client-only new-conversation draft has no materialized session
- **THEN** no trace is created for it and history commands do not expose the retained previous session's trace

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
- **WHEN** a trace cannot be safely opened for writing
- **THEN** the client reports recording unavailable, continues the session, and does not fall back to a hidden alternate location

#### Scenario: Truncated final JSONL record
- **WHEN** a crash leaves valid complete records followed by an incomplete line
- **THEN** inspection exposes the valid records with a warning and further recording does not concatenate new JSON onto the partial line

#### Scenario: Delayed result after session replacement
- **WHEN** a history query finishes after the active session changes
- **THEN** it does not update the new page, insert the old path into its composer, or trigger a stale clipboard write

### Requirement: Local history command behavior
`/history` SHALL behave identically to `/history show`. `/history show` SHALL open the full-screen history view. `/history path` SHALL insert the existing current-session trace's absolute path as editable composer text without sending a prompt, appending a transcript message, or executing a provider command. The path result SHALL use the ordinary character/atomic-block-safe insertion policy and SHALL not overwrite edits made while the request was pending. A no-trace or unmaterialized-session request SHALL show a clear unavailable state without returning the old session's path or creating a fictitious trace. Unknown subcommands or extra arguments SHALL yield local usage feedback without provider forwarding.

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

### Requirement: Title-free time-scaled history presentation
The history page SHALL display a time axis with the represented start at the left endpoint and end at the right, distinguish operation kinds by consistent theme-aware colors plus a text legend, and place overlapping operations in separate lanes. It SHALL omit the prototype's title, session masthead, and summary-header section. Exact start/end, operation summary, status and duration SHALL remain readable in a chronological record list without hover. Width SHALL encode elapsed time rather than event count; operations shorter than a terminal cell SHALL use explicitly qualified markers rather than false expanded durations. Long disconnected gaps SHALL remain identifiable; any time compression SHALL be disclosed. Empty/zero-width time ranges, tiny terminals, color-disabled output, and incomplete traces SHALL remain intelligible and bounded. History paging SHALL preserve its own viewport and SHALL not alter the conversation's viewport.

#### Scenario: Inspect a short edit beside a long command
- **WHEN** the trace contains a 20ms edit and a 12s command
- **THEN** the time display preserves their relative scale or marks the subcell edit explicitly, while the list shows both exact durations

#### Scenario: Enter with split Preview active
- **WHEN** the user opens history from the normal split screen
- **THEN** the history page replaces that screen without an extra history title and the hidden conversation/Preview remain restorable
