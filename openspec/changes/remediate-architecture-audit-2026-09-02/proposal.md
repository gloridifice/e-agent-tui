## Why

The shared frontend runtime has retained correct package layering, but duplicated runner policy and several large coordination modules now concentrate change risk in both executable adapters and the frontend core. The architecture guard also scans only root Rust modules, so it cannot reliably protect the nested module structure that the remediation will introduce.

## What Changes

- Make the Rust architecture graph scanner recursive and resolve nested `crate`, `self`, and `super` module dependencies so acyclicity checks cover the complete production module tree.
- Move normalized scheduling policy, inbound fairness rules, streaming-delta classification, and common `UiAction` execution behind one provider-neutral `e_tui::runtime` API used by both executable runners.
- Keep DSH and Pi transports, async composition roots, platform paths, clipboard implementations, Windows FFI, and provider lifecycle behavior in their owning adapters.
- Split the Pi anti-corruption adapter into request, response, session, model, tool, and extension-UI responsibilities while preserving the `PiAdapter` facade.
- Split frontend runtime controller and reduction code by interaction and lifecycle responsibility while preserving the `RuntimeController` and `RuntimeState` public facades.
- Preserve wire messages, persistent formats, user-visible behavior, frame pacing, lock-release discipline, and terminal restoration semantics.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `acyclic-client-architecture`: Require architecture checks to include nested production modules and resolve relative module dependencies rather than scanning only source-root files.
- `testable-runtime-ports`: Require both adapters to consume one shared scheduling policy and common frontend-action executor while retaining provider-specific transports and external effect implementations.

## Impact

- Affects `crates/e-tui/src/runtime/`, especially controller, state, policy, ports, and common action execution.
- Affects `crates/e-dsh/src/main.rs`, `crates/e-pi/src/main.rs`, and `crates/e-pi/src/adapter*`.
- Affects `crates/e-dsh/tests/architecture.rs` and focused runtime/adapter regression tests.
- Updates the current English client architecture documentation and the modified OpenSpec capabilities.
- Does not change `bridge/`, `bridge/protocol-contract.json`, the DSH wire protocol, Pi RPC protocol, persisted configuration formats, or user-facing commands.
