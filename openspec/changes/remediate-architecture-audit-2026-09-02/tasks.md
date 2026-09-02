## 1. Make the architecture guard recursive

- [x] 1.1 Replace root-only Rust source discovery in `crates/e-dsh/tests/architecture.rs` with recursive discovery and canonical module IDs for flat files, `mod.rs`, and nested files.
- [x] 1.2 Resolve `crate`, `self`, repeated `super`, and grouped import paths to the longest discovered production module target without treating module declarations as dependency edges.
- [x] 1.3 Add nested scanner fixtures covering absolute imports, relative imports, grouped imports, test-only references, and a reported multi-module cycle.
- [x] 1.4 Run the recursive graph against `e-dsh`, `e-pi`, and `e-tui`, retain the existing package/rendering boundary assertions, and verify the focused architecture test passes.

## 2. Establish one shared runner policy

- [ ] 2.1 Add `e_tui::runtime::policy` with the animation minimum, inbound count/time budgets, deadline wait helper, animation interval calculation, budget admission, and normalized streaming-delta classification.
- [ ] 2.2 Replace the DSH runner's local scheduling constants and helpers with the shared policy while leaving its WebSocket selection loop unchanged.
- [ ] 2.3 Replace the Pi runner's local scheduling constants and helpers with the shared policy while leaving its RPC process selection loop unchanged.
- [ ] 2.4 Add focused policy tests for idle deadlines, shared fairness limits, animation clamping, and streaming-delta classification.

## 3. Share ordered frontend action execution

- [ ] 3.1 Define the narrow agent-send seam and shared `EffectExecution` result in `e_tui::runtime`, keeping provider transport errors representable without provider types.
- [ ] 3.2 Implement a common executor that processes `UiAction` values sequentially through agent and external-effect ports and returns ordered completion facts, quit state, and fatal errors.
- [ ] 3.3 Switch `crates/e-dsh/src/main.rs` to the common executor while retaining DSH request conversion, bridge transport, concrete effects, and shutdown ownership.
- [ ] 3.4 Switch `crates/e-pi/src/main.rs` to the common executor while retaining the Pi request channel, process lifecycle, concrete effects, and shutdown ownership.
- [ ] 3.5 Add scripted-port regressions for interleaved agent/effect ordering, persistence and clipboard failures, Preview completion, draw admission, quit, and transport failure after frontend locks are released.

## 4. Decompose the Pi anti-corruption adapter

- [ ] 4.1 Convert `crates/e-pi/src/adapter.rs` into an `adapter/` module tree while preserving the public `PiAdapter`, `AdapterOutput`, `new`, `startup_commands`, `request`, and `record` API.
- [ ] 4.2 Extract outbound `AgentRequest` routing and RPC response/error dispatch without changing command IDs, ordering, or unsupported-action behavior.
- [ ] 4.3 Extract session attach, title derivation/deduplication, snapshot, and session-list projection into one cohesive session responsibility with focused tests.
- [ ] 4.4 Extract model catalog, thinking-level, model-selection, and pending-effort state with focused model regressions.
- [ ] 4.5 Extract extension-UI request/answer correlation and tool lifecycle/Preview/result-deduplication responsibilities with focused regressions.
- [ ] 4.6 Keep sequence allocation and cross-domain event ordering in the `PiAdapter` facade, verify raw Pi JSON does not escape the adapter, and run the scoped e-pi adapter tests.

## 5. Decompose the frontend runtime controller

- [ ] 5.1 Convert `runtime/controller.rs` into a private module tree with a stable public `RuntimeController` facade and shared context/borrow types.
- [ ] 5.2 Move pointer, resize, selection, Reading, and ordinary terminal-key handling into the terminal controller responsibility without changing input precedence.
- [ ] 5.3 Move session, catalog, interaction, and normalized agent-error handling into the agent controller responsibility.
- [ ] 5.4 Move composer actions, Input Page actions, approval handling, effect completion, config reload, and queued-prompt dispatch into their owning controller responsibilities.
- [ ] 5.5 Relocate existing controller tests by behavior and run focused lock-release, session-switch, queued-prompt, Input Page, Reading, selection, and Preview-race regressions.

## 6. Decompose normalized runtime reduction

- [ ] 6.1 Convert `runtime/state.rs` into a private state module tree while preserving the public `RuntimeState` facade and its single `TuiApp` root.
- [ ] 6.2 Move timeline/snapshot/history orchestration and assistant reduction into cohesive state submodules without duplicating projector decisions.
- [ ] 6.3 Move tool, activity, retry, lifecycle, and workflow mutation application into cohesive state submodules that continue to consume existing projection effects.
- [ ] 6.4 Move spinner, settle, reveal-adjacent animation, and Preview enrichment helpers into their owning state responsibility without changing deadlines or cache invalidation.
- [ ] 6.5 Verify `TuiApp` lifecycle models remain the sole transcript, Preview, session, interaction, catalog, and render owners, then run focused reducer/rendering regressions.

## 7. Document and verify the remediated boundary

- [ ] 7.1 Update `docs/subsystem/client/architecture.md` in English for the shared runner policy, common action executor, recursive architecture guard, and internal runtime/adapter decomposition.
- [ ] 7.2 Run `cargo test -p e-dsh --test architecture`, focused e-tui runtime tests, focused e-pi adapter/process tests, and smoke both `dshe` and `pie` runner startup paths.
- [ ] 7.3 Run `cargo fmt --all` and `cargo clippy --all-targets`, resolving all findings introduced by the large refactor.
- [ ] 7.4 Run the full workspace test suite and confirm no bridge, wire-contract, persisted-format, command, keybinding, or user-visible rendering change was introduced.
