## MODIFIED Requirements

### Requirement: Reusable package boundary
The Rust workspace SHALL provide `e-dsh` and `e-pi` executable adapter packages and an `e-tui` library package. Adapter dependencies MUST point toward the provider-neutral frontend, and `e-tui` MUST NOT import DSH or Pi wire messages, raw host event names, DSH setup/profile behavior, Pi process behavior, filesystem persistence, clipboard implementations, or process-control infrastructure. While the existing reducer/controller migration remains incomplete, `e-pi` MAY reuse executable-side runtime infrastructure from the `e-dsh` library, but it MUST NOT use DSH protocol, bridge, setup, launcher, or WebSocket modules; this transitional adapter-to-adapter dependency MUST NOT introduce Pi or DSH types into `e-tui`.

#### Scenario: Build the DSH executable
- **WHEN** the workspace builds the `dshe` binary
- **THEN** `e-dsh` composes DSH infrastructure with the public `e-tui` API through the existing adapter boundary

#### Scenario: Build the Pi executable
- **WHEN** the workspace builds the `pie` binary
- **THEN** `e-pi` composes Pi RPC infrastructure with normalized `e-tui` events and actions without starting or importing DSH bridge behavior

#### Scenario: Check kernel neutrality
- **WHEN** architecture tests scan production imports in `e-tui`
- **THEN** no DSH protocol, Pi RPC, WebSocket, child-process, `e-dsh`, or `e-pi` module is reachable from the library

### Requirement: Normalized event and action contract
`e-tui` SHALL accept normalized `AgentEvent` facts and frontend `InputEvent` values through synchronous state transitions and SHALL return complete, owned `UiAction` values, dirty state, and any next deadline. An action executor MUST be able to execute every action without borrowing `TuiApp`, and each agent adapter MUST translate its native protocol before invoking the frontend contract.

#### Scenario: Project a DSH tool event
- **WHEN** `e-dsh` receives a raw DSH tool frame
- **THEN** its adapter maps the frame to a normalized tool capability and constructs an `AgentEvent` before `e-tui` updates presentation state

#### Scenario: Project a Pi tool event
- **WHEN** `e-pi` receives a Pi RPC tool record
- **THEN** its adapter maps the record to a normalized tool capability and constructs an `AgentEvent` before `e-tui` updates presentation state

#### Scenario: Execute an external effect
- **WHEN** a UI transition requests persistence, clipboard access, agent I/O, Preview resolution, drawing, or exit
- **THEN** it returns an owned `UiAction` whose executable-specific executor runs after any UI state guard has been released

#### Scenario: Complete asynchronous work
- **WHEN** an action executor finishes or fails asynchronous work
- **THEN** the result returns to `e-tui` as an event fact rather than mutating UI state from the background task
