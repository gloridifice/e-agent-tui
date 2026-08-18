# event-display-surfaces Specification

## Purpose
TBD - created by archiving change unify-event-display-framework. Update Purpose after archive.
## Requirements
### Requirement: Four shared event display surfaces
The client SHALL represent user-visible event output through exactly four shared surface contracts: status-bearing activity rows, ordinary transcript blocks, padded content cards, and input-area accessories. Event-specific presentation MAY specialize content inside a surface but SHALL NOT bypass the shared transcript or input-area layout paths.

#### Scenario: Existing event presentation is classified
- **WHEN** an existing Thinking row, tool row, assistant message, lifecycle notice, user message, approval, question, or queued prompt is projected
- **THEN** it is represented by one of the four shared surfaces without an event-specific top-level rendering path

#### Scenario: Rich event composes shared surfaces
- **WHEN** an event requires both lifecycle status and a detailed content body
- **THEN** the client composes an activity row with a content card instead of introducing another base surface

### Requirement: Activity lifecycle contract
An activity row SHALL have stable identity, a label, a lifecycle state, optional summary and timing, and optional parent identity. Its lifecycle state SHALL distinguish running, waiting, success, failure, and cancellation, and an update SHALL modify the existing row rather than append a duplicate.

#### Scenario: Tool result settles its activity
- **WHEN** a `tool/result` correlates with a displayed `tool/call`
- **THEN** the existing activity row transitions from running to success or failure with its final bounded summary and timing

#### Scenario: Nested activity identifies its parent
- **WHEN** a Code Mode subcall or workflow member starts under a displayed parent activity
- **THEN** its activity row records the parent identity and renders at a deterministic terminal depth

#### Scenario: Interrupted activity stops animating
- **WHEN** a turn ends while an activity remains running
- **THEN** the activity transitions to failure or cancellation and no longer schedules breathing animation

### Requirement: Ordinary transcript block contract
A transcript block SHALL render non-status content using a declared content format and tone. It SHALL support plain notices, Markdown source, reasoning content, and bounded unknown-surface fallback content while preserving the original copy source.

#### Scenario: Assistant final message renders Markdown
- **WHEN** a finalized assistant message contains text blocks
- **THEN** one ordinary transcript block renders the combined text as Markdown and exposes the original Markdown to copy mode

#### Scenario: Turn outcome renders a notice
- **WHEN** a turn ends as aborted, blocked, interrupted, max-tokens, or error
- **THEN** an ordinary transcript block presents the appropriate notice or error detail without a running status

#### Scenario: Unknown append surface is bounded
- **WHEN** the client receives an unknown event carrying append-surface metadata
- **THEN** it renders a bounded fallback transcript block that identifies the event without dumping an unbounded payload

### Requirement: Padded content card contract
A content card SHALL render content with shared padding, background, wrapping, and copy provenance rules. User messages, structured injected context, and textual attachment placeholders SHALL use this contract.

#### Scenario: User message preserves current card layout
- **WHEN** a direct user message is displayed
- **THEN** it uses the shared content card with the configured gutter, continuous background across wrapping, and verbatim copy text

#### Scenario: Structured context is distinguishable
- **WHEN** an injected user-role message declares an instructions, catalog, snapshot, relay, or recall context form
- **THEN** it renders as a context content card whose role is visually distinguishable from a direct user card

#### Scenario: Image block has terminal fallback
- **WHEN** a displayed message contains an image block and no terminal image protocol is supported
- **THEN** the content card renders a stable textual attachment description without exposing raw image bytes

### Requirement: Input accessory layout and focus
Input-area accessories SHALL share one height calculation and render above the input editor with deterministic priority, collapse, and focus rules. Blocking approval and question accessories SHALL receive input ahead of informational accessories, and exactly one blocking accessory SHALL handle a key event.

#### Scenario: Blocking question receives focus
- **WHEN** a question accessory and informational todo or queue accessories are present
- **THEN** the question accessory is focused and handles answer-navigation keys

#### Scenario: Informational accessories coexist
- **WHEN** queued prompts and todo or goal information are present with sufficient terminal height
- **THEN** each accessory renders in deterministic priority order above the input editor

#### Scenario: Small terminal applies height budget
- **WHEN** all accessories cannot fit while retaining the minimum transcript and editor heights
- **THEN** lower-priority informational accessories collapse before a blocking accessory or the editor is removed

### Requirement: Shared layout owns copy provenance
Every transcript display surface SHALL produce visible rows and copy provenance through the same width-aware layout process. Atomic Markdown, table, code, Mermaid, and future rich-detail bodies SHALL retain their existing atomic copy semantics.

#### Scenario: Wrapped card copy follows visible layout
- **WHEN** a padded content card wraps across multiple terminal rows
- **THEN** copy-mode row navigation uses exactly those rendered rows while copied text comes from the card’s original source

#### Scenario: Activity composition has stable spacing
- **WHEN** adjacent activity rows or an activity row with a detail card are rendered
- **THEN** spacing is produced by the shared layout and copy mode does not independently infer gaps

### Requirement: Incremental rendering behavior
Streaming updates SHALL dirty only the affected tail presentation when no structural change occurs. Appends, removals, replacements, accessory height changes, and activity hierarchy changes SHALL invalidate the appropriate structural cache, and rendering SHALL continue to clone only the visible window.

#### Scenario: Text delta updates only the tail
- **WHEN** a text delta appends to the current streaming assistant block
- **THEN** the transcript cache remains structurally valid and marks only its tail dirty

#### Scenario: Surface replacement is structural
- **WHEN** a surface replacement removes and inserts display items
- **THEN** the transcript cache is structurally invalidated once and the viewport anchor is preserved where possible

#### Scenario: Animation remains dirty-driven
- **WHEN** no activity, stream, or settle transition is animating and no state changed
- **THEN** the client does not schedule a redraw solely for the display framework
