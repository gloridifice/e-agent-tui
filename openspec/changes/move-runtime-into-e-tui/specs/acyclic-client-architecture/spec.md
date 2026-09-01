## ADDED Requirements

### Requirement: Adapter dependencies do not point sideways
The workspace architecture guards SHALL reject direct or transitive production dependencies from `e-pi` to `e-dsh` and from `e-dsh` to `e-pi`. Shared frontend runtime behavior MUST be reached through `e-tui` rather than through an adapter compatibility facade.

#### Scenario: Pi imports the transitional DSH library
- **WHEN** `e-pi` declares `e-dsh` in its manifest or imports the transitional `e` library name
- **THEN** the architecture check fails and identifies the forbidden adapter-to-adapter edge

#### Scenario: Both adapters use the shared runtime
- **WHEN** architecture checks inspect the completed workspace graph
- **THEN** `e-dsh` and `e-pi` each point to `e-tui`, and no shared runtime source is owned by one adapter for reuse by the other

### Requirement: Shared runtime modules remain acyclic
Production modules under `e_tui::runtime` SHALL follow one-way dependencies from adapter-facing coordination toward controller, scheduling, ports, terminal, and input leaves. Runtime modules MUST NOT introduce a strongly connected component with `TuiApp`, rendering, interaction, or provider adapters.

#### Scenario: Runtime leaf reaches into an adapter
- **WHEN** a runtime input, terminal, scheduler, or port module imports an `e-dsh` or `e-pi` module
- **THEN** the architecture check fails with the forbidden path

#### Scenario: Runtime module cycle is introduced
- **WHEN** runtime extraction creates a multi-module strongly connected component
- **THEN** the architecture check fails and reports every module in the cycle

## MODIFIED Requirements

### Requirement: Kernel boundary is mechanically enforced
Architecture guards SHALL reject DSH protocol names, Pi RPC names, provider transport types, and raw provider event names in `e-tui`. They SHALL keep bridge/setup/launcher behavior in `e-dsh`, Pi process/RPC behavior in `e-pi`, and provider-specific path, persistence, clipboard, Preview I/O, and async effect execution in the owning executable adapter. Provider-neutral terminal coordination, controller logic, scheduling, and runtime port contracts SHALL be owned by `e_tui::runtime`.

#### Scenario: Protocol type leaks into the frontend library
- **WHEN** an `e-tui` production module imports a DSH `ServerMessage` or `ClientMessage`, a Pi RPC DTO, or either adapter's protocol module
- **THEN** the architecture test fails and identifies the forbidden import

#### Scenario: Provider behavior leaks into the shared runtime
- **WHEN** `e_tui::runtime` imports DSH bridge/setup/launcher behavior, Pi child-process behavior, or provider-specific persistence policy
- **THEN** the architecture test fails and identifies the provider boundary violation

#### Scenario: Provider-neutral terminal code is shared
- **WHEN** terminal routing, frame scheduling, synchronized drawing, or terminal restoration is used by both executables
- **THEN** its production implementation is reachable through `e_tui::runtime` rather than either adapter package
