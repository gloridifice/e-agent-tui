## ADDED Requirements

### Requirement: Reusable package boundary
The Rust workspace SHALL provide an `e-dsh` executable package and an `e-tui` library package, and the only dependency between them SHALL point from `e-dsh` to `e-tui`. `e-tui` MUST NOT import DSH wire messages, raw DSH event names, DSH setup/profile behavior, filesystem persistence, clipboard implementations, or process-control infrastructure.

#### Scenario: Build the DSH executable
- **WHEN** the workspace builds the `dshe` binary
- **THEN** `e-dsh` composes DSH infrastructure with the public `e-tui` API through the `e-dsh -> e-tui` dependency

#### Scenario: Check kernel neutrality
- **WHEN** architecture tests scan production imports in `e-tui`
- **THEN** no DSH protocol module, WebSocket message type, or `e-dsh` module is reachable from the library

### Requirement: Normalized event and action contract
`e-tui` SHALL accept normalized `AgentEvent` facts and frontend `InputEvent` values through synchronous state transitions and SHALL return complete, owned `UiAction` values, dirty state, and any next deadline. An action executor MUST be able to execute every action without borrowing `TuiApp`.

#### Scenario: Project a DSH tool event
- **WHEN** `e-dsh` receives a raw DSH tool frame
- **THEN** its adapter maps the frame to a normalized tool capability and constructs an `AgentEvent` before `e-tui` updates presentation state

#### Scenario: Execute an external effect
- **WHEN** a UI transition requests persistence, clipboard access, agent I/O, Preview resolution, drawing, or exit
- **THEN** it returns an owned `UiAction` whose executor runs after any UI state guard has been released

#### Scenario: Complete asynchronous work
- **WHEN** an action executor finishes or fails asynchronous work
- **THEN** the result returns to `e-tui` as an event fact rather than mutating UI state from the background task

### Requirement: Lifecycle-based state ownership
`TuiApp` SHALL assign session, timeline, interaction, Reading View, Preview, catalog, and render state to explicit lifecycle owners. A migration facade MAY forward operations temporarily, but production state MUST NOT maintain mirrored transcript, session, or Preview domain stores.

#### Scenario: Add semantic reading metadata
- **WHEN** the timeline is projected for Reading View
- **THEN** the reading model indexes the existing timeline and provenance identities without copying messages into a second transcript store

#### Scenario: Extract one lifecycle
- **WHEN** state fields move from the compatibility facade into a lifecycle model
- **THEN** the lifecycle model becomes their sole owner before the phase is considered complete

#### Scenario: Move rendering across the package boundary
- **WHEN** the Ratatui renderer moves from `e-dsh` into `e-tui`
- **THEN** every lifecycle state it consumes is already owned by `e-tui`, and the renderer does not depend on an `e-dsh` facade or a host-facing compatibility trait

### Requirement: Layered rendering composition
`e-tui` rendering SHALL compose as `Screen -> Pane -> Region -> Component`. Components MUST NOT depend on Regions, Panes, or the Screen; Regions MUST NOT perform I/O; and rendering MUST NOT mutate domain text.

#### Scenario: Render the complete terminal screen
- **WHEN** `TuiApp` renders a frame
- **THEN** the Screen computes rectangles, Panes arrange Regions, and Regions render view models with downward-only Component dependencies

#### Scenario: Reuse a card shell
- **WHEN** both a main-pane Region and Preview Region need a themed shell
- **THEN** they may depend on the same leaf Component without the Component importing either Region

### Requirement: Main-pane visual continuity
The extracted main pane SHALL preserve the current transcript surfaces, Markdown styling, card treatment, composer/Input Page presentation, status/title rows, spacing, theme semantics, hidden hardware cursor behavior, and copy provenance at equivalent effective widths. Introducing the Preview pane MUST NOT redesign these main-pane elements; width-driven rewrapping is the expected exception.

#### Scenario: Complete behavior-preserving rendering extraction
- **WHEN** the single-column renderer has been moved into the layered modules
- **THEN** TestBackend characterization assertions for line counts, colors, spacing, content, and cursor behavior remain equivalent

#### Scenario: Render the main pane beside Preview
- **WHEN** a wide terminal uses the two-pane layout
- **THEN** the main pane uses the established surface components and theme semantics at its narrower effective width rather than a sidebar-driven restyle
