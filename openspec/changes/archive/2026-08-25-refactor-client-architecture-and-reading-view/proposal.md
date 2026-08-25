## Why

The Rust client has strong runtime and rendering behavior, but its DSH-specific infrastructure, UI state, and rendering composition still live in one package, making the next major interaction model difficult to add safely or reuse with another agent kernel. This change establishes explicit package and state boundaries first, then adds a semantic Reading View and shared Preview sidebar without regressing the current main-column presentation or performance contracts.

## What Changes

- Split the Rust workspace into an `e-dsh` executable package and a kernel-neutral `e-tui` library, with dependency direction `e-dsh -> e-tui`.
- Introduce normalized `AgentEvent`, `InputEvent`, `UiAction`, and update-result contracts so DSH protocol and effects remain outside the UI library.
- Decompose UI state by lifecycle and reorganize rendering as `Screen -> Pane -> Region -> Component` while preserving the current main-column visual style.
- Add a responsive Preview sidebar shared by normal mode and Reading View; its visual design may introduce sidebar-specific components or reuse existing primitives, but must remain visually coherent with the current theme.
- Add a width-independent semantic Reading Document with stable Block and Item identities, width-dependent geometry, source provenance, and Preview references.
- Add Reading View with Block and Item navigation, semantic copy, draft preservation, cursor-driven Preview selection, and asynchronous Preview resolution with stale-result protection.
- Replace row-oriented Copy Mode after Reading View reaches feature parity, while preserving atomic source-copy behavior for Markdown, code, tables, and Mermaid.
- Preserve the current event-driven runtime, bounded queues, short lock discipline, incremental transcript rendering, history anchors, display surfaces, and performance redlines throughout the migration.
- **BREAKING** Change the public reading/copy interaction from `Ctrl+B` Copy Mode to Reading View, using `Ctrl+V` when it passes the supported-terminal compatibility gate or one documented alternate otherwise.
- **BREAKING** Change the source-install package path from `client` to `crates/e-dsh` when the workspace split lands, unless a documented temporary compatibility path is retained.

## Capabilities

### New Capabilities
- `kernel-neutral-agent-tui`: Defines the reusable `e-tui` package boundary, normalized event/action contracts, state ownership, and layered rendering composition.
- `unified-preview-pane`: Defines the responsive Preview sidebar, normal/latest and reading/cursor policies, fallback presentation, cache, and asynchronous resolution behavior.
- `semantic-reading-view`: Defines semantic Blocks and Items, stable identities, navigation, highlighting, scrolling, copying, input routing, and Copy Mode replacement.

### Modified Capabilities
- `acyclic-client-architecture`: Extends architecture guards across both crates and enforces downward rendering dependencies and the absence of DSH imports in `e-tui`.
- `event-display-surfaces`: Requires existing display surfaces and main-column styling to survive the pane/region/component refactor and provide semantic Block/Item annotations.
- `input-page`: Preserves blocking-page precedence and composer draft state when Reading View is introduced and updates the non-page interaction roster from Copy Mode to Reading View.
- `single-display-projection`: Adds a semantic reading index over the single transcript path without creating a duplicate transcript store, while preserving projection and history semantics.
- `terminal-render-performance`: Extends bounded layout, cache, and frame-work requirements to the two-pane layout, Preview updates, and Reading cursor movement.
- `testable-runtime-ports`: Recasts controller effects as owned `UiAction` values across the crate boundary and adds asynchronous Preview completion races to deterministic tests.

## Impact

- Rust workspace layout, package manifests, build-script paths, assets, install commands, and profiling feature forwarding.
- Most modules currently under `client/src`, especially runtime, model, projection, input, copy, cache, transcript layout, rendering, terminal orchestration, setup, launcher, config, and protocol handling.
- Public keyboard behavior, help text, UI layout, config/theme schemas, TestBackend expectations, architecture guards, and performance benchmarks.
- No bridge or wire-protocol change is required unless later implementation discovers missing Preview data that cannot be derived by the DSH adapter.
