# dsh-event-projection Specification

## Purpose
TBD - created by archiving change unify-event-display-framework. Update Purpose after archive.
## Requirements
### Requirement: Typed event classification boundary
Every received DSH session event SHALL be translated at the protocol boundary into a typed event kind plus event-level sequence, top-level timestamp, source sequence, and surface-operation metadata. Application reducers SHALL consume typed projection effects and SHALL NOT inspect arbitrary JSON payloads.

#### Scenario: Top-level timestamp is retained
- **WHEN** a DSH event contains its timestamp in the event-level `time` field
- **THEN** the typed event exposes that timestamp for duration and ordering calculations

#### Scenario: Unknown event remains bounded and typed
- **WHEN** the protocol receives an unrecognized event type
- **THEN** it creates an unknown typed event containing bounded identity and surface metadata without passing its raw payload into the reducer

### Requirement: Projection effects separate display from state mutation
The event projector SHALL classify each typed event into one or more explicit effects: display append/update, surface mutation, page/session state, input accessory state, or ignore. The classification SHALL be exhaustive for every typed event kind.

#### Scenario: Title updates page state only
- **WHEN** a `session/title` event is projected
- **THEN** it updates the session title page state without creating a transcript item or invalidating the transcript cache

#### Scenario: Audit record is explicitly ignored
- **WHEN** an audit-only or reconstruction-only event is projected
- **THEN** the projector returns an explicit ignore classification and creates no user-visible item

### Requirement: DSH surface append and replace semantics
The client SHALL maintain ordered DSH surface-node identity and apply append or replace operations before final presentation. A replace operation SHALL remove the display ownership of every shadowed surface node and insert the replacement at the replaced range’s surface position.

#### Scenario: Compaction replacement removes old content
- **WHEN** a replacement user message shadows an earlier surface range
- **THEN** display items owned by that range are removed and the replacement is shown once at the range position

#### Scenario: Replaced tool result removes paired activity
- **WHEN** a surface replacement shadows a `tool/result`
- **THEN** the activity row owned by its paired `tool/call` and `tool/result` lifecycle is removed from the effective transcript

#### Scenario: Repeated replacements preserve order
- **WHEN** a later replacement shadows a surface node created by an earlier replacement
- **THEN** the final surface order matches DSH replacement semantics without duplicating either summary

### Requirement: Surface replacement survives backward history paging
The client SHALL retain shadowed sequence metadata independently of the currently loaded display window. Older events fetched after a replacement has been observed SHALL not reintroduce shadowed content, and viewport anchoring SHALL account only for effective newly inserted rows.

#### Scenario: Older shadowed page is suppressed
- **WHEN** the initial snapshot contains a replacement and a later backward page contains its shadowed source events
- **THEN** those source events do not create display items

#### Scenario: Effective older page preserves viewport
- **WHEN** a backward page contains both shadowed and still-effective events
- **THEN** only effective rows are prepended and the visible viewport moves by exactly their rendered row count

### Requirement: Core conversation event projection
Core user, assistant, tool, turn, and step events SHALL be correlated into the shared display surfaces without duplicate lifecycle items. Assistant text and reasoning SHALL be accumulated per turn and step; step boundary events SHALL provide correlation without generating empty transcript rows.

#### Scenario: Streaming assistant is finalized
- **WHEN** text and reasoning chunks for one step are followed by `assistant/message`
- **THEN** the existing streaming transcript presentation is finalized from the assembled content without duplicate text

#### Scenario: Direct and injected messages differ
- **WHEN** two `user/message` events have direct-user and structured-context sources
- **THEN** the direct message becomes a user content card and the context message becomes the matching context card or notice

#### Scenario: Empty step remains invisible
- **WHEN** a step starts and ends without user-visible assistant or tool content
- **THEN** no empty transcript display item is created

### Requirement: Tool result status and timing are accurate
Tool activity SHALL derive timing from event-level timestamps and failure from both the tool-result block’s `isError` state and structured event error identity. Result summaries SHALL describe only data retained by bridge trimming and SHALL NOT label a truncated tail count as the complete output count.

#### Scenario: Model-facing tool error is shown as failure
- **WHEN** a tool-result content block has `isError: true` without an event-level error object
- **THEN** the correlated activity finishes in the failure state

#### Scenario: Duration uses event timestamps
- **WHEN** tool call and result events contain ordered top-level timestamps
- **THEN** the displayed duration is their non-negative difference

#### Scenario: Truncated output count is qualified
- **WHEN** the bridge sends only a bounded tail of a large tool result
- **THEN** the activity does not claim that the retained tail line count is the full output line count

### Requirement: Extended activity-family projection
Model retries, durable commands, nested Code Mode dispatches, and workflow events SHALL project to correlated activity rows. Start and settlement records SHALL update stable identities; nested records SHALL retain parent relationships.

#### Scenario: Retry chain updates one logical activity
- **WHEN** multiple retry records for one model step arrive
- **THEN** one retry activity reflects the current attempt, waiting/running state, bounded failure reason, and eventual settlement

#### Scenario: Durable command survives reconnect
- **WHEN** `command/run` and `command/done` exist in snapshot or history
- **THEN** the reconstructed command activity matches its live presentation and is not duplicated by an immediate command-result frame

#### Scenario: Code dispatch is nested
- **WHEN** a `tool/code-dispatch-start` and matching settlement identify a parent root call
- **THEN** one child activity row is created under that parent and then settled

#### Scenario: Workflow member lifecycle is correlated
- **WHEN** workflow run and member start/end records arrive
- **THEN** run and member activity rows retain stable identities, hierarchy, and terminal outcomes

### Requirement: Todo and input-context projection
The latest `todo/write` whole-list snapshot SHALL drive a todo input accessory, and a new turn SHALL retire the previous standing todo according to DSH semantics. Pending approvals, questions, queued prompts, goal state, and plan mode SHALL use the shared accessory layout rather than transcript-specific rendering.

#### Scenario: Todo write replaces the whole accessory state
- **WHEN** a later `todo/write` event arrives
- **THEN** its complete list replaces the earlier todo accessory contents rather than appending entries

#### Scenario: New turn clears standing todo
- **WHEN** a `turn/start` occurs after a todo snapshot
- **THEN** the previous todo accessory is retired unless a new todo snapshot arrives

#### Scenario: Blocking and informational state coexist
- **WHEN** a pending question arrives while todo and queued-prompt accessories are visible
- **THEN** the question receives focus while the informational states remain available under the accessory height policy

### Requirement: Rich turn outcomes and compaction presentation
Turn outcomes SHALL preserve structured error detail and distinguish aborted, blocked, interrupted, error, max-tokens, and completed reasons. Compaction lifecycle MAY create activity/card presentation, but its visual presentation SHALL remain independent from mandatory surface replacement.

#### Scenario: Structured turn error is retained
- **WHEN** a turn ends with an error containing message and code
- **THEN** the failure activity and notice expose bounded human-readable detail rather than a fixed generic string

#### Scenario: Max-token outcome is distinguishable
- **WHEN** a turn ends with `max-tokens`
- **THEN** a warning transcript block identifies the output-token limit outcome

#### Scenario: Compaction remains correct without a card renderer
- **WHEN** compaction presentation is unavailable but a valid surface replacement arrives
- **THEN** the transcript is still replaced correctly without displaying duplicate old content

### Requirement: History roster supports reconstructable displays
The canonical wire contract SHALL include every event family required to reconstruct supported display and accessory state after reconnect. The bridge SHALL retain event ordering and surface metadata, apply bounded payload policies, and omit records classified as audit-only unless another supported projection requires them.

#### Scenario: Live and reconstructed projection agree
- **WHEN** the same supported event sequence is consumed live and from snapshot/history
- **THEN** both paths produce equivalent settled display items and accessory state, excluding intentionally live-only animation transitions

#### Scenario: History roster changes remain synchronized
- **WHEN** a supported event family is added to snapshot/history
- **THEN** `bridge/protocol-contract.json`, generated protocol documentation, bridge tests, and Rust contract constants agree on the protocol version and roster

### Requirement: Unsupported replacement fails safely
An unknown or malformed surface replacement whose effective ordering cannot be reconstructed SHALL not be silently treated as an append. The client SHALL expose a bounded compatibility error while preserving connection control and already valid transcript state.

#### Scenario: Unknown replacement is rejected
- **WHEN** an unknown event carries a replacement operation that the projector cannot apply safely
- **THEN** the client reports a compatibility error and does not append the event as ordinary content

### Requirement: Typed tool Preview normalization boundary
The DSH adapter SHALL normalize tool-call arguments and supported result presentation metadata into bounded provider-neutral Preview facts. `e-tui` reducers and renderers MUST NOT inspect raw DSH tool names, raw argument JSON fields, or opaque result metadata. Unsupported, malformed, or bridge-trimmed optional presentation metadata SHALL fail soft to a typed path, JSON, or no-specialized-preview fallback.

#### Scenario: Known call schema is normalized
- **WHEN** DSH emits a read, view, create, filesystem-search, command, bash, pwsh, edit, replace, or insert call with valid arguments
- **THEN** the adapter emits the corresponding typed Preview seed while retaining existing capability and transcript projection facts

#### Scenario: DSH edit result carries diff metadata
- **WHEN** a tool result contains valid bounded `meta.diffs` entries with path, old text, and new text
- **THEN** the protocol/adapter boundary emits ordered typed mutation hunks without computing a new diff

#### Scenario: Metadata is malformed or trimmed
- **WHEN** result metadata is absent, malformed, or replaced by the bridge's bounded metadata fallback
- **THEN** the event still settles its activity and Preview degrades without exposing arbitrary JSON to `e-tui`

#### Scenario: Unknown tool arguments are normalized
- **WHEN** DSH emits an unsupported tool name
- **THEN** the adapter emits the original display name and bounded normalized JSON source rather than requiring renderer-side DSH schema inspection

### Requirement: Tool Preview facts survive event reconstruction
Typed tool Preview seeds, result output, metrics inputs, mutation metadata, and truncation state SHALL be derived consistently from live, snapshot, and history events. Cross-page result-before-call ordering SHALL preserve enough bounded typed state to finalize the same Preview when the call is later observed.

#### Scenario: Live and replayed command agree
- **WHEN** the same command call/result pair is reduced live and from a snapshot
- **THEN** both paths produce equivalent Preview name, command text, final metrics, truncation qualification, and secondary output

#### Scenario: Older page supplies a missing call
- **WHEN** a result and its typed Preview facts are staged from a newer page and the matching call arrives in an older page
- **THEN** the adapter/projector correlates them by call ID without reparsing raw event JSON in the renderer

