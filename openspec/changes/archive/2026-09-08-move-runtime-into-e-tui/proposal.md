## Why

`e-pi` currently depends on the `e-dsh` library only to reuse provider-neutral frontend runtime code, which creates an adapter-to-adapter dependency and compiles DSH infrastructure into the Pi package boundary. The shared runtime should live as an `e_tui::runtime` module so both executable adapters depend only on the frontend package without introducing another workspace crate.

## What Changes

- Add an `e_tui::runtime` module that owns provider-neutral runtime coordination, terminal event routing and lifecycle, frame scheduling, effect-port contracts, and shared platform frontend helpers.
- Move the reusable controller/state facade and other provider-neutral runtime implementation out of `e-dsh`; split any mixed modules so DSH protocol conversion and DSH-only effects remain in `e-dsh`.
- Make `e-dsh` and `e-pi` composition roots consume the same public `e_tui::runtime` API while retaining their own agent transports, configuration/session persistence, and provider-specific effect implementations.
- Remove the `e-pi -> e-dsh` Cargo dependency and prevent future adapter-to-adapter imports.
- Preserve current DSH and Pi behavior, terminal restoration guarantees, event fairness, frame pacing, Preview completion ordering, and lock-release-before-effect invariants.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `kernel-neutral-agent-tui`: Replace the transitional adapter-to-adapter dependency with a shared runtime module inside `e-tui` while keeping DSH and Pi protocol types outside the frontend package.
- `testable-runtime-ports`: Move provider-neutral runtime ownership and terminal coordination into `e_tui::runtime`, with executable adapters supplying provider-specific ports and composition.
- `acyclic-client-architecture`: Enforce `e-dsh -> e-tui` and `e-pi -> e-tui` as the only internal package edges and keep the new runtime module acyclic and provider-neutral.

## Impact

- Affects `crates/e-tui/src`, especially a new `runtime` module and shared runtime/platform dependencies.
- Refactors `crates/e-dsh/src/{runtime,runtime_ports,terminal_runtime,vt_input,win_input,model,profile}.rs` where code is provider-neutral or currently acts as a compatibility facade.
- Refactors `crates/e-dsh/src/main.rs` and `crates/e-pi/src/main.rs` to compose adapters through `e_tui::runtime`.
- Removes `e-dsh` from `crates/e-pi/Cargo.toml`; DSH bridge, protocol, launcher, setup, and Pi RPC/process modules remain in their owning adapter packages.
- Updates architecture tests and focused runtime/rendering regressions; no wire protocol or persisted-format change is intended.
