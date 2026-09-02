## ADDED Requirements

### Requirement: Runner scheduling policy has one shared owner
`e_tui::runtime` SHALL own the normalized animation minimum, inbound item and time budgets, deadline helpers, budget admission, and streaming-delta classification used by every executable adapter. Adapter runners MUST retain control of their provider inbound streams but MUST NOT maintain independent copies of these frontend scheduling decisions.

#### Scenario: Scheduling policy changes
- **WHEN** a frontend fairness limit, animation minimum, or normalized streaming admission rule changes
- **THEN** both DSH and Pi runners consume the changed value or behavior from the same `e_tui::runtime` implementation without parallel adapter edits

#### Scenario: Provider inbound streams remain independent
- **WHEN** DSH receives WebSocket frames and Pi receives RPC process records
- **THEN** each executable keeps its own provider `tokio::select!` and normalization path while applying the shared budget and deadline policy

#### Scenario: Idle and backlog behavior stays equivalent
- **WHEN** either adapter is idle or its provider continuously supplies inbound events
- **THEN** it preserves zero periodic idle wakeups and yields at the same shared count or time budget for terminal input and expired deadlines

### Requirement: Common frontend actions use one ordered executor
`e_tui::runtime` SHALL provide one provider-neutral executor for ordered `UiAction` sequences. The executor MUST use adapter-supplied ports for agent transport, configuration, clipboard, Preview, and clock work; it MUST return normalized completion facts, quit state, and fatal transport errors without borrowing frontend state or constructing provider infrastructure.

#### Scenario: Equivalent action is executed by either adapter
- **WHEN** DSH and Pi receive the same persistence, clipboard, Preview, draw, or quit action
- **THEN** both use the shared executor and produce the same normalized completion and scheduling semantics while their own port performs external work

#### Scenario: Agent requests preserve order
- **WHEN** an action sequence interleaves agent requests with local external effects
- **THEN** the shared executor invokes the supplied agent port and other effect ports in original sequence order

#### Scenario: External work occurs after state release
- **WHEN** the controller returns an action that requires await, filesystem, clipboard, Preview, or provider transport work
- **THEN** the adapter calls the shared executor only after releasing frontend state guards

#### Scenario: Provider infrastructure stays outside the frontend package
- **WHEN** the shared executor dispatches an agent or external-effect action
- **THEN** DSH/Pi transport types, platform paths, clipboard implementations, Windows FFI, and process or service lifecycle remain implemented in the owning executable adapter
