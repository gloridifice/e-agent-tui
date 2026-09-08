# pi-agent-frontend Specification

## Purpose
TBD - created by archiving change add-pi-agent-frontend. Update Purpose after archive.
## Requirements
### Requirement: Pi-backed executable startup
The workspace SHALL provide package `e-pi` with executable artifact `pie`. `pie` MUST start the native Pi CLI in RPC mode for the launch working directory, MUST preserve Pi's native configuration/resource/session locations, and MUST NOT start or require the DSH bridge.

#### Scenario: Start in a configured project
- **WHEN** the user runs `pie` in a project with native Pi settings and resources
- **THEN** `pie` starts `pi --mode rpc` for that cwd and Pi applies its native settings, context, trust, skills, prompts, packages, and extensions

#### Scenario: Pi is unavailable
- **WHEN** `pie` cannot execute a compatible `pi` command
- **THEN** startup fails before terminal takeover with actionable install and version guidance

#### Scenario: One-run trust override
- **WHEN** the user launches `pie` with an approve or no-approve option
- **THEN** the matching native Pi trust override is passed to the RPC process without writing a second trust store

### Requirement: Strict bounded RPC transport
`e-pi` SHALL communicate with Pi through LF-delimited JSON objects on child stdin/stdout. It MUST split only on LF, accept an optional trailing CR, bound each record and internal queue, keep stderr separate, and terminate/reap the child on frontend shutdown.

#### Scenario: JSON contains a Unicode line separator
- **WHEN** an RPC JSON string contains U+2028 or U+2029
- **THEN** the transport retains it inside the same record and does not split before LF

#### Scenario: Malformed or oversized stdout record
- **WHEN** Pi stdout emits malformed JSON or a record above the configured limit
- **THEN** `pie` stops consuming that stream and reports a fatal protocol diagnostic instead of presenting partial state

#### Scenario: Frontend exits
- **WHEN** the user quits `pie` or terminal initialization fails after child startup
- **THEN** the Pi child is closed and reaped within a bounded shutdown path

### Requirement: Authoritative startup and replacement snapshots
After startup and every successful session replacement, `e-pi` MUST query Pi state, messages, commands, models, and supported thinking levels. Complete state/messages SHALL be authoritative, while streaming deltas SHALL remain presentation updates.

#### Scenario: Start with an existing session
- **WHEN** RPC startup opens a persisted Pi session
- **THEN** `pie` projects its current metadata and complete active messages before applying later live events

#### Scenario: Replace the active session
- **WHEN** Pi successfully creates or switches the session
- **THEN** `pie` clears old session-scoped interaction state and refreshes authoritative state, messages, commands, and model metadata

### Requirement: Prompt and lifecycle integration
The adapter SHALL translate ordinary prompts, steering while busy, abort, deferred new-session first prompts, compaction, and session switching to native Pi RPC commands. A deferred first prompt MUST be sent only after successful new-session replacement and MUST be recoverable after replacement failure.

#### Scenario: Send while idle
- **WHEN** the user submits ordinary text while Pi is idle
- **THEN** `pie` sends a Pi `prompt` command and projects the resulting user, assistant, and tool lifecycle

#### Scenario: Send while running
- **WHEN** the user submits ordinary text while Pi is streaming
- **THEN** `pie` sends it with Pi steering behavior rather than issuing an invalid unqualified prompt

#### Scenario: Materialize a deferred new session
- **WHEN** the user enters `/new` and then submits the first prompt
- **THEN** `pie` waits for successful `new_session`, refreshes the attached state, and then sends the complete retained prompt exactly once

#### Scenario: New-session replacement fails
- **WHEN** Pi rejects or fails the correlated `new_session` request
- **THEN** `pie` returns the retained prompt to the composer and displays the failure

### Requirement: Normalized transcript and tool projection
`e-pi` MUST translate Pi user, assistant, reasoning, tool, retry, compaction, and error records into provider-neutral `AgentEvent`/`TimelineRecord` facts before frontend reduction. Authoritative completion records MUST settle or repair delta-assembled state.

#### Scenario: Stream text and reasoning
- **WHEN** Pi emits `text_delta` or `thinking_delta`
- **THEN** `pie` appends the corresponding normalized assistant chunk while retaining complete semantic content for the final message

#### Scenario: Execute a known Pi tool
- **WHEN** Pi emits tool start and end records for a built-in read, edit, write, search, or shell tool
- **THEN** `pie` projects the matching provider-neutral capability, readable summary, result state, and event-supplied Preview data without reading files or computing a diff

#### Scenario: Execute an extension tool
- **WHEN** a Pi extension runs a tool unknown to `e-pi`
- **THEN** `pie` uses the generic tool surface and preserves bounded textual/JSON arguments and output

#### Scenario: Pi adds an unknown event
- **WHEN** RPC emits an event type not required by the supported protocol version
- **THEN** `pie` ignores or reports it as a bounded diagnostic without desynchronizing subsequent records

### Requirement: Native model and command catalogs
`pie` SHALL populate the existing model, thinking-effort, skill, prompt, and extension-command UI from Pi RPC query results. Model and thinking changes MUST be performed by Pi and then refreshed from authoritative state.

#### Scenario: Open model selection
- **WHEN** the user opens `/model`
- **THEN** `pie` queries available Pi models and displays them grouped by provider with the current selection

#### Scenario: Select reasoning effort
- **WHEN** the selected model supports multiple Pi thinking levels and the user chooses one
- **THEN** `pie` sends `set_thinking_level` and refreshes the current model/thinking state

#### Scenario: Invoke a discovered command
- **WHEN** the user selects a Pi extension command, prompt template, or skill command from completion
- **THEN** `pie` submits the slash invocation through Pi prompt expansion rather than expanding the resource in Rust

### Requirement: Native session resume roster
`pie` SHALL display current-project native Pi sessions in the existing Resume page. It MAY read bounded documented JSONL metadata but MUST treat session files as read-only and MUST use Pi RPC for switching or creating sessions.

#### Scenario: List current-project sessions
- **WHEN** the user opens `/resume`
- **THEN** `pie` lists matching native session ids/paths, latest native names or first-user-message fallbacks, and creation timestamps without modifying a file

#### Scenario: Session file is malformed
- **WHEN** a candidate native session file has an invalid header or malformed metadata
- **THEN** `pie` skips that candidate, emits a bounded diagnostic, and continues listing valid sessions

#### Scenario: Select a session
- **WHEN** the user selects a Resume row
- **THEN** `pie` sends the corresponding native session path to Pi `switch_session` and rebuilds the frontend from authoritative queries

### Requirement: RPC-compatible Extension UI
`pie` SHALL support Pi RPC dialog requests for select, confirm, input, and editor and SHALL return a matching `extension_ui_response`. It SHALL support notify and editor-text updates through provider-neutral frontend interactions. It MUST NOT claim Pi TUI mode or attempt to execute TypeScript custom components.

#### Scenario: Extension requests a selection
- **WHEN** Pi emits an `extension_ui_request` with method `select`
- **THEN** `pie` opens a ratatui Input Page and returns the selected value or cancellation with the same request id

#### Scenario: Extension requests confirmation
- **WHEN** Pi emits an `extension_ui_request` with method `confirm`
- **THEN** `pie` opens a ratatui confirmation interaction and returns the confirmed boolean or cancellation

#### Scenario: Extension updates composer text
- **WHEN** Pi emits `set_editor_text`
- **THEN** `pie` replaces the visible ordinary composer text without mutating a hidden non-editing Input Page draft

#### Scenario: Extension uses Pi-TUI-only UI
- **WHEN** an extension depends on `ctx.mode === "tui"` custom components, headers, footers, or raw terminal APIs
- **THEN** Pi observes RPC mode and `pie` follows Pi RPC's documented unsupported or degraded behavior rather than simulating Pi TUI objects

### Requirement: Native credentials with external login management
`pie` SHALL use credentials resolved by Pi from native auth files and environment variables. The first release MUST NOT write a parallel credential store and MUST direct interactive login management to native `pi /login`.

#### Scenario: Credentials already exist
- **WHEN** native Pi can resolve credentials for the selected model
- **THEN** `pie` prompts and streams without copying API keys into its own configuration

#### Scenario: Authentication is missing
- **WHEN** Pi reports that the selected provider lacks valid authentication
- **THEN** `pie` displays actionable guidance to run native `pi /login` or configure the provider environment variable

