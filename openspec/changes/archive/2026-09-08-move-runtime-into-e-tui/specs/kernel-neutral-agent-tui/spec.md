## MODIFIED Requirements

### Requirement: Reusable package boundary
The Rust workspace SHALL provide `e-dsh` and `e-pi` executable adapter packages and an `e-tui` frontend library package. Both adapters MUST depend directly on `e-tui`, and neither adapter package may depend on the other. `e-tui` SHALL expose provider-neutral state, rendering, interaction, and runtime APIs, including terminal coordination under `e_tui::runtime`; it MUST NOT import DSH or Pi wire messages, raw provider event names, DSH setup/profile behavior, Pi child-process behavior, provider-specific filesystem paths or persistence policy, clipboard implementations, or agent transport implementations.

#### Scenario: Build the DSH executable
- **WHEN** the workspace builds the `dshe` binary
- **THEN** `e-dsh` composes DSH infrastructure with `e_tui::runtime` and the normalized frontend API through the `e-dsh -> e-tui` dependency

#### Scenario: Build the Pi executable
- **WHEN** the workspace builds the `pie` binary
- **THEN** `e-pi` composes Pi RPC infrastructure with `e_tui::runtime` and the normalized frontend API without importing or building through `e-dsh`

#### Scenario: Check package direction
- **WHEN** architecture tests inspect workspace manifests and production imports
- **THEN** the only internal adapter edges point from `e-dsh` and `e-pi` toward `e-tui`, with no adapter-to-adapter dependency

#### Scenario: Check kernel neutrality
- **WHEN** architecture tests scan production imports in `e-tui`
- **THEN** no DSH protocol module, WebSocket message type, or `e-dsh` module is reachable from the library

#### Scenario: Check provider neutrality
- **WHEN** architecture tests scan production imports in `e-tui`
- **THEN** no DSH protocol, Pi RPC, WebSocket, child-process, `e-dsh`, or `e-pi` module is reachable from the library

### Requirement: Normalized event and action contract
`e-tui` SHALL accept normalized `AgentEvent` facts and frontend `InputEvent` values through synchronous state transitions and SHALL return complete, owned `UiAction` values, dirty state, and any next deadline. An action executor MUST be able to execute every action without borrowing `TuiApp`, and each agent adapter MUST translate its native protocol before invoking `e_tui::runtime`.

#### Scenario: Project a DSH tool event
- **WHEN** `e-dsh` receives a raw DSH tool frame
- **THEN** its adapter maps the frame to a normalized tool capability and constructs an `AgentEvent` before the shared runtime updates frontend state

#### Scenario: Project a Pi tool event
- **WHEN** `e-pi` receives a Pi RPC tool record
- **THEN** its adapter maps the record to a normalized tool capability and constructs an `AgentEvent` before the shared runtime updates frontend state

#### Scenario: Execute an external effect
- **WHEN** a UI transition requests persistence, clipboard access, agent I/O, Preview resolution, drawing, or exit
- **THEN** it returns an owned `UiAction` whose executable-specific executor runs after any frontend state guard has been released

#### Scenario: Complete asynchronous work
- **WHEN** an action executor finishes or fails asynchronous work
- **THEN** the result returns to `e_tui::runtime` as a normalized completion fact rather than mutating frontend state from the background task
