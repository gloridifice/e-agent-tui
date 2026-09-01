## 1. Establish the shared runtime boundary

- [x] 1.1 Add `e_tui::runtime` submodules for controller, scheduler, ports, terminal, and input, and expose only provider-neutral public types.
- [x] 1.2 Add architecture guards for the two adapter-to-`e-tui` package edges, forbidden adapter imports, provider wire names, and runtime-module cycles.

## 2. Move shared runtime primitives

- [x] 2.1 Move the duplicated frame scheduler and its admission/deadline tests into `e_tui::runtime::scheduler`, then use it from both runners.
- [x] 2.2 Move terminal event routing and the pure VT parser with their regression tests into `e_tui::runtime::input`.
- [x] 2.3 Move terminal ownership, synchronized frame submission, and provider-neutral frame/I/O metrics into `e_tui::runtime::terminal`, preserving idempotent restoration.
- [x] 2.4 Define shared terminal/effect/clock port contracts and scripted test ports under `e_tui::runtime::ports`, leaving provider transports and concrete external effects in adapters.

## 3. Extract normalized controller and state

- [x] 3.1 Split DSH protocol conversion and provider-specific error/command handling from normalized runtime transitions.
- [x] 3.2 Move normalized controller, terminal interaction handling, and shared runtime state to `e_tui::runtime::controller` over `TuiApp`, `AgentEvent`, `AgentRequest`, and owned actions.
- [x] 3.3 Move controller, lock-release, Preview race, queued-prompt, input precedence, and terminal restoration tests to their new owning modules.

## 4. Adopt the runtime in both adapters

- [x] 4.1 Update `dshe` composition and effect execution to use `e_tui::runtime` while retaining DSH transport, setup, persistence paths, clipboard, and Preview I/O in `e-dsh`.
- [x] 4.2 Update `pie` composition and effect execution to use `TuiApp` and `e_tui::runtime`, adding Pi-owned persistence, clipboard, Preview, and Windows native-input shims where required.
- [x] 4.3 Remove every `e::` import from `e-pi`, delete its `e-dsh` Cargo dependency, and remove obsolete DSH-side shared-runtime facades or duplicate implementations.

## 5. Verify behavior and document the boundary

- [x] 5.1 Run scoped architecture, scheduler, input, controller, terminal, and rendering regression tests for the moved modules.
- [x] 5.2 Build and smoke-check both `dshe` and `pie`, including idle scheduling, bounded inbound fairness, synchronized drawing/restoration, Preview completion, and Windows input paths.
- [x] 5.3 Update the current English client architecture documentation to describe `e_tui::runtime`, direct adapter dependencies, and adapter-owned external effects.
- [x] 5.4 Run `cargo fmt --all`, `cargo clippy --all-targets`, and OpenSpec validation; resolve all failures and mark the change implementation-complete.
