## ADDED Requirements

### Requirement: Shared runtime ports serve every adapter
`e_tui::runtime` SHALL define provider-neutral terminal, effect, and clock seams that can be implemented by each executable adapter and replaced by scripted tests. Shared handlers MUST consume normalized frontend values and MUST NOT construct DSH, Pi, filesystem, clipboard, or process implementations directly.

#### Scenario: Execute the same frontend action through either adapter
- **WHEN** DSH and Pi runners receive the same owned clipboard, persistence, Preview, draw, or exit action
- **THEN** both runners dispatch it through the shared runtime contract while their adapter-owned port performs the external operation

#### Scenario: Script an external failure
- **WHEN** a scripted adapter port returns a clipboard, persistence, or Preview failure
- **THEN** the shared runtime produces the same normalized visible completion behavior without accessing a production service

### Requirement: Adapter runners preserve equivalent scheduling policy
Both executable runners MUST use the shared frame scheduler and the same interactive, content, animation, and idle admission rules. Each runner SHALL retain control of its provider inbound stream and MUST yield after the configured item or time budget so terminal input and expired deadlines are not starved.

#### Scenario: Idle operation
- **WHEN** no terminal input, provider event, animation, or frame deadline is pending
- **THEN** neither runner periodically wakes or draws through a fixed ticker

#### Scenario: Provider backlog competes with input
- **WHEN** either provider continuously supplies inbound events while terminal input arrives
- **THEN** the runner yields at the shared count or time budget and processes input and expired frames

## MODIFIED Requirements

### Requirement: Preview resolution races are deterministic through ports
The shared runtime port model SHALL execute deferred Preview work outside frontend state guards and return completion facts carrying request ID, key, revision, and result. Scripted tests MUST be able to order target changes and completions arbitrarily, and each executable adapter MUST be able to supply the external resolver without changing controller behavior.

#### Scenario: Late completion is scripted
- **WHEN** a test requests Preview A, selects B, then delivers completion A
- **THEN** A may enter cache but the visible state remains targeted at B without any real asynchronous task

#### Scenario: Completion requests a frame
- **WHEN** a matching Preview completion changes visible state
- **THEN** the returned dirty/action state schedules drawing through the shared frame scheduler rather than drawing directly

### Requirement: Main loop remains the async and terminal composition root
`e_tui::runtime` SHALL own provider-neutral terminal lifecycle, terminal event routing, synchronized frame submission, frame scheduling, and controller mechanics without creating an async executor or agent process. Each executable adapter SHALL remain the async composition root for its transport, bounded channels, inbound fairness budget, external effect implementations, and provider lifecycle. Calling `TuiApp` state, input, and render APIs in isolation MUST NOT initialize a terminal, executor, filesystem, clipboard, or provider process.

#### Scenario: Frontend core is updated in isolation
- **WHEN** a unit test calls `TuiApp::update`, `handle_input`, and render methods without constructing `e_tui::runtime` terminal facilities
- **THEN** no Tokio runtime, terminal lifecycle, filesystem, clipboard, DSH service, or Pi process is required

#### Scenario: Construct a production runner
- **WHEN** either executable starts interactive operation
- **THEN** it creates the shared terminal/runtime facilities and composes them with its own transport and external effect ports

#### Scenario: Restore after an adapter failure
- **WHEN** either provider transport or effect executor terminates with a fatal error
- **THEN** the shared terminal owner performs the same idempotent restoration path before the executable exits
