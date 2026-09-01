## Context

The frontend split is incomplete. `e-tui` owns normalized state, projection, rendering, and interactions, but `e-dsh` still owns a mixed runtime layer containing provider-neutral controller logic, terminal routing/lifecycle, frame metrics, platform input, and DSH protocol handling. The `pie` binary therefore imports the `e-dsh` library as `e` for `AppState`, `RuntimeController`, runtime ports, `TerminalOwner`, profiling, theme loading, clipboard access, and Preview resolution. It does not use the DSH service, but the Cargo edge still couples the adapters and causes DSH build/runtime infrastructure to sit on Pi's dependency path.

Both binaries also carry equivalent frame-scheduler and effect-execution structure. The extraction must preserve the event-driven loop, bounded inbound work, independent deadlines, synchronized terminal output, Windows raw VT input behavior, and the rule that state guards are released before external work.

## Goals / Non-Goals

**Goals:**

- Establish `e_tui::runtime` as the one shared frontend runtime API without creating an `e-runtime` crate.
- Make both executable packages depend directly on `e-tui` and remove every `e-pi -> e-dsh` import.
- Keep the shared runtime provider-neutral: it operates on `AgentEvent`, `AgentRequest`, `InputEvent`, `UiAction`, and frontend-owned state, never DSH or Pi wire values.
- Share controller, terminal, input, scheduling, and effect-port behavior while preserving deterministic scripted tests.
- Keep provider transports, process launch, path selection, persistence policy, and protocol error translation in their adapter packages.
- Preserve visible behavior, performance budgets, terminal restoration, and existing persisted formats.

**Non-Goals:**

- Creating another workspace package or publishing a separately versioned runtime crate.
- Moving DSH bridge/setup/launcher code or Pi RPC/process code into `e-tui`.
- Unifying DSH and Pi wire protocols, configuration directories, session stores, or process lifecycles.
- Redesigning the UI, changing keybindings, or changing wire contracts.
- Making `TuiApp` itself asynchronous or requiring a terminal to test state/projection/rendering APIs.

## Decisions

### 1. Put the shared runtime behind `e_tui::runtime`

The target package graph is:

```text
e-dsh ──→ e-tui ←── e-pi
```

`e-tui` will expose a `runtime` module with focused submodules rather than a second crate:

```text
e_tui::runtime
├── controller    normalized event/input transitions
├── scheduler     frame admission and independent deadlines
├── ports         terminal/effect/clock contracts
├── terminal      terminal ownership and atomic frame submission
└── input         terminal routing and VT stream parsing
```

This keeps the runtime and the state/render API versioned together and avoids a third package whose only consumers are the two workspace adapters. A separate `e-runtime` crate was considered, but it would add release/dependency overhead without creating a useful independent boundary.

### 2. Separate frontend runtime mechanics from provider composition

The shared module owns behavior that is identical for every agent adapter:

- normalized terminal-event routing and interaction precedence;
- controller transitions over frontend-owned state;
- frame scheduling, dirty-priority coalescing, and animation deadlines;
- terminal setup/restoration and synchronized frame submission;
- terminal I/O metrics and provider-neutral profiling hooks;
- runtime port traits and reusable scripted implementations;
- pure VT parsing and the safe, provider-independent portion of terminal event acquisition.

Each executable remains the composition root and owns:

- DSH WebSocket or Pi JSONL process transport;
- native protocol-to-`AgentEvent` and `AgentRequest`-to-native-command conversion;
- provider-specific startup, shutdown, and error guidance;
- `%APPDATA%` path selection, config/session persistence, clipboard and deferred file Preview effect implementations;
- the Tokio `select!` loop and bounded provider-inbound queue.

The runners may call shared scheduler/controller/effect-dispatch helpers, but `e-tui` will not start an executor or an agent process. This preserves adapter control over transport fairness while eliminating duplicated frontend mechanics.

### 3. Split `e-dsh::runtime` by type boundary before moving code

The existing module cannot move wholesale because it imports `ClientMessage`, `ServerMessage`, wire protocol constants, profile names, and bridge adapters. Migration will first separate normalized controller paths from DSH translation:

- DSH frames are converted to complete normalized events in `e-dsh::bridge::adapter` before entering `e_tui::runtime`.
- Frontend actions leave the shared controller as `AgentRequest`; `e-dsh` converts them to `ClientMessage`, and `e-pi` converts them to RPC commands.
- DSH protocol mismatch, authentication, launcher, and bridge-close wording stays in `e-dsh` and is represented to the frontend through normalized fatal/notice events or actions.
- Provider-neutral local-command handling moves with the controller; commands that require a provider capability produce normalized requests instead of constructing DSH messages.

This avoids preserving an `e-dsh` compatibility facade inside the new module.

### 4. Use `TuiApp` as the shared state owner

`e-pi` will stop importing `e-dsh::model::AppState`. State needed by both runners must be owned by `TuiApp` or by a provider-neutral runtime state type under `e_tui::runtime`. Raw DSH replay/protocol state remains adapter-private and may temporarily use an `e-dsh` facade, but that facade cannot appear in the public shared runtime API.

This makes the package boundary real: the shared controller receives only frontend state and normalized values. It also removes the need for re-export facades such as `e::model`, `e::runtime`, and `e::theme` in Pi code.

### 5. Keep external effects behind adapter-supplied ports

`e_tui::runtime::ports` defines the contracts and completion types, but handlers do not construct filesystem, clipboard, transport, or process implementations. Configuration persistence carries a complete snapshot; Preview work carries a complete request; all asynchronous completions return as normalized facts.

Common parsing, bounds checks, and effect-dispatch policy may live in `e-tui`, while actual path access and platform services remain supplied by the executable. DSH and Pi may use small adapter implementations with different directories/session keys without duplicating controller behavior.

### 6. Preserve the safe frontend core and isolate Windows native calls

`e-tui` currently forbids unsafe code. The pure VT parser and cancellation-safe input state can move into `e_tui::runtime::input`, but direct Windows console/key-state calls will remain in minimal executable-side platform shims passed into the shared input source. This preserves the safety guard while allowing both adapters to reuse the parser and acquisition logic. The existing bracketed-paste and modifier behavior is a migration invariant, not an opportunity to fall back to crossterm's incomplete Windows paste path.

### 7. Enforce the target boundary mechanically

Architecture tests will assert:

- `e-pi/Cargo.toml` has no `e-dsh` dependency and production Pi sources contain no `e::` imports;
- neither adapter imports the other's modules;
- `e_tui::runtime` contains no DSH/Pi wire names, bridge modules, child-process commands, or provider-specific paths;
- the `e-tui` production module graph remains acyclic;
- terminal/effect operations occur after frontend state guards are released.

Behavioral tests move with their owner. Shared scheduler, routing, terminal restoration, and controller tests belong in `e-tui`; provider translation and composition tests remain in each adapter.

## Risks / Trade-offs

- **The current runtime module mixes normalized and DSH-specific behavior extensively** → Extract by semantic seams and keep DSH conversion tests in place before deleting facades; do not perform a blind file move.
- **Moving terminal ownership broadens `e-tui` beyond a pure state/render library** → Keep it in an explicit `runtime` namespace; `TuiApp` remains synchronous and independently testable, and process/persistence/clipboard implementations remain outside.
- **Two runners may drift while still owning their Tokio loops** → Share scheduler and effect-dispatch policy, and add parity tests for idle wakeups, frame priority, terminal restoration, and bounded inbound yielding.
- **Windows input can regress during relocation** → Retain the pure parser fixtures and platform byte table, keep native calls in narrow shims, and smoke bracketed paste plus modified Enter/Backspace in both binaries.
- **Temporary duplicate code may exist during staged migration** → Allow duplication only while both binaries build at an intermediate checkpoint; remove the old implementation and re-export facades before marking the change complete.
- **Profiling environment names are DSH-branded** → Preserve current variables as compatibility aliases while moving generic counters; defer any user-facing rename unless a separate compatibility plan is approved.

## Migration Plan

1. Add the `e_tui::runtime` namespace and architecture guards, then move pure scheduler, routing, VT parser, profiling, and terminal transaction code with focused tests.
2. Move normalized controller/state behavior to `e-tui`; leave explicit DSH translation wrappers in `e-dsh` and update `dshe` to use the shared API.
3. Define shared ports and adapt DSH production effects without changing paths, session persistence, clipboard behavior, or Preview bounds.
4. Convert `pie` to `TuiApp`/shared runtime APIs, add Pi-owned effect/platform adapters, and remove all `e::` imports.
5. Remove `e-dsh` from `crates/e-pi/Cargo.toml`, delete obsolete facades/duplication, and run scoped architecture, runtime, rendering, and both-binary smoke checks.
6. Update current architecture documentation because the package ownership invariant changes. Rollback is a commit-level revert to the existing transitional dependency; no data or protocol migration is involved.

## Open Questions

- Whether the two Tokio loops should remain thin adapter-owned compositions permanently or be unified later behind a generic inbound-stream runner. This change shares their policy and mechanics but does not require a generic transport loop.
- Whether provider-neutral filesystem helpers for theme discovery and bounded Preview reads belong in `e_tui::runtime` or should remain tiny adapter implementations over shared parsing/bounds utilities. The deciding constraint is keeping path and persistence policy out of `e-tui`.
