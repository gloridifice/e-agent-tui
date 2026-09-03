## Context

`e-dsh` and `e-pi` now depend directly on the provider-neutral `e-tui` package, and the production package graph has the intended direction. The move left two smaller architectural weaknesses: both executable composition roots still encode the same scheduling and common action-execution policy, while `RuntimeController`, `RuntimeState`, and `PiAdapter` retain broad implementation files with several independent reasons to change.

The current architecture test uses a root-file-only Rust module graph. It therefore cannot verify the nested `runtime`, `projection`, `ui`, and adapter modules that this remediation will create. The change must preserve the frontend's event-driven performance, state-lock discipline, provider boundaries, public facades, and existing behavior.

## Goals / Non-Goals

**Goals:**

- Make the acyclicity guard cover every nested production Rust module in all three workspace packages.
- Give scheduling constants, fairness admission, deadline helpers, streaming-delta classification, and common `UiAction` execution one provider-neutral owner.
- Keep each executable adapter responsible for its transport, process/service lifecycle, platform paths, concrete external effects, and async composition.
- Split the Pi adapter and frontend runtime implementation by stable change axis without changing their public entry points.
- Preserve action ordering, lock release before external work, frame pacing, terminal restoration, Preview race handling, and user-visible behavior.

**Non-Goals:**

- Changing the DSH wire contract, Pi RPC contract, persisted configuration, commands, keybindings, or rendering behavior.
- Moving filesystem, clipboard, Windows FFI, provider transport, or process management into `e-tui`.
- Sharing the complete provider `tokio::select!` loops or introducing a generic provider runtime framework.
- Adding another workspace support crate solely to remove small amounts of adapter-boundary glue.
- Refactoring the Node.js bridge.

## Decisions

### 1. Protect the refactor with a recursive source module graph first

The architecture test will recursively discover Rust module files and assign canonical IDs: `foo.rs` and `foo/mod.rs` represent `foo`, while `foo/bar.rs` represents `foo::bar`. Dependency extraction will resolve `crate`, `self`, and repeated `super` prefixes against the current module and select the longest discovered module prefix as the target node. Test-only modules remain excluded, and module declarations are not dependency edges.

The implementation will extend the existing lightweight scanner instead of adding a Rust parser dependency. This keeps the architecture gate fast and dependency-free; dedicated nested fixtures will characterize grouped imports and relative paths so scanner limitations remain visible.

Alternative considered: use `syn` to parse every source file. This gives more syntactic accuracy but adds compile cost and still requires custom module resolution and cfg handling, so it does not remove the difficult part of the check.

### 2. Share policy and action orchestration, not provider event loops

A new `e_tui::runtime::policy` module will own animation minimums, inbound count/time budgets, deadline waiting, animation interval calculation, budget admission, and normalized streaming-delta classification. Both runners will consume these values directly.

A new `e_tui::runtime::executor` module will execute ordered `UiAction` sequences through existing `UiActionPorts` plus a narrow `AgentRequestPort`. It will return a shared `EffectExecution` containing completion facts, quit state, and an optional fatal transport error. The executor may await adapter-supplied port futures, but it will not create a Tokio runtime, construct infrastructure, or know DSH/Pi transport types.

The two executable loops remain separate because their inbound sources and provider lifecycle semantics differ materially. They will continue to own `tokio::select!`, provider event normalization, bounded queues, startup, and shutdown.

Alternative considered: extract a generic complete runner. This would require a large provider abstraction and would hide important DSH/Pi lifecycle differences, creating more accidental complexity than it removes.

### 3. Keep narrow platform duplication at the adapter boundary

Windows native-key sampling, application directory selection, Pi temporary image files, DSH image bytes, and concrete theme/config filesystem operations remain adapter-owned. Pure policy and normalized action semantics are shared, but infrastructure details are not moved into `e-tui`.

Alternative considered: introduce an `e-platform` workspace package. The current amount of reusable platform code is too small to justify a fourth package and would weaken the simple `adapter -> e-tui` dependency rule.

### 4. Preserve `PiAdapter` as the anti-corruption facade

`crates/e-pi/src/adapter.rs` will become an `adapter/` module tree. `PiAdapter::new`, `startup_commands`, `request`, and `record` remain the public API. Internal responsibilities will be separated into request routing, RPC response handling, session/title projection, model projection, tool projection, and extension-UI relay.

State that has one lifecycle will move into cohesive subobjects such as session, model, tool, and extension-UI state. Sequence allocation and cross-domain ordering remain in the facade. Raw Pi JSON values remain inside the adapter boundary.

Alternative considered: split only the file while retaining one flat state object. That reduces file size but not change propagation, so the design uses small lifecycle state objects where ownership is unambiguous.

### 5. Preserve runtime facades while splitting implementation modules

`RuntimeController` remains the adapter-facing synchronous controller. Its implementation will move into private terminal, agent, input, effect, and context modules. `RuntimeState` remains the normalized state facade; timeline, assistant, tool, activity, and animation application will move into private state submodules.

The existing separation remains authoritative: `projection/*` decides projection effects, while `runtime/state/*` applies those effects to lifecycle owners. The split must not create a second transcript, Preview, session, or render store.

Alternative considered: replace both facades with a new framework or trait hierarchy. That would expand the public API and make a behavior-preserving remediation unnecessarily risky.

### 6. Refactor in independently verifiable slices

The implementation order is: recursive architecture guard, shared policy, shared executor, Pi adapter decomposition, controller decomposition, and state decomposition. File movement and behavior changes will not be combined in the same step. Existing tests will move with their owning behavior, and new tests will cover only the new architecture and shared-runner risks.

## Risks / Trade-offs

- [The lightweight import scanner can misread unusual Rust syntax] → Cover canonical, grouped, `self`, and multi-level `super` imports with nested fixtures and fail with module IDs that make scanner errors diagnosable.
- [A shared executor can reorder actions] → Process the original `Vec<UiAction>` sequentially and retain completion ordering; transport is represented as an ordered port call rather than a separately drained queue.
- [Moving controller code can break lock discipline] → Keep public methods synchronous, retain scoped guards, and run lock-release and queued-prompt regression tests after each slice.
- [Splitting state can duplicate projection decisions] → Keep projector output types unchanged and allow state submodules only to apply mutations to existing lifecycle owners.
- [Pi state decomposition can break cross-event correlation] → Keep request IDs, sequence IDs, and cross-domain dispatch in `PiAdapter`; move state only when one subobject has clear ownership.
- [Extra modules can become shallow wrappers] → Require each new module to own a coherent behavior set and avoid one-function delegation modules.

## Migration Plan

1. Extend the architecture scanner and add nested-cycle fixtures before moving production code.
2. Introduce shared runtime policy and switch both runners without changing their event loops.
3. Introduce the common executor and switch one adapter at a time, preserving action order and error text.
4. Decompose `PiAdapter` behind its unchanged public API and migrate tests by responsibility.
5. Decompose `RuntimeController`, then `RuntimeState`, keeping each intermediate revision buildable.
6. Update current English architecture documentation and run focused tests after each stage.
7. Finish with `cargo fmt --all`, `cargo clippy --all-targets`, full workspace tests, and smoke checks for both executables.

Every stage can be rolled back independently because no wire or persisted migration is involved.

## Open Questions

- Whether the shared executor should accept a dedicated `AgentRequestPort` or extend `UiActionPorts`; implementation should choose the smaller API that preserves ordered transport errors without combining unrelated concrete ports.
- Whether lifecycle state objects in `PiAdapter` provide enough ownership benefit for every domain; domains with no retained state may remain pure helper modules.
