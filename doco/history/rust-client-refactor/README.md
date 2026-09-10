# Architecture refactor record

> Status: Historical
> Authority: Non-normative. These files retain staged rationale and gate evidence; they do not define the current implementation.

This directory records the completed Rust client migration and its baselines. For current behavior, use the [Rust client architecture](../../subsystem/client/architecture.md), source, tests, and generated contracts. The [archived design draft](../../archive/dsh-tui-design-v0.5.md) is context only.

## Plan documents

- [architecture.md](architecture.md): target crate boundaries, agent kernel contract, state ownership, rendering layers, layout, and runtime model.
- [reading-view.md](reading-view.md): the Reading View, semantic Block and Item model, unified Preview pane, navigation, copying, and preview resolution.
- [migration.md](migration.md): phased migration, current-to-target module map, test gates, risks, and documentation work.
- [baseline.md](baseline.md): pre-migration scoped tests, visual characterization fixtures, install/setup behavior, and performance measurements.
- [terminal-binding-gate.md](terminal-binding-gate.md): why `Ctrl+V` failed the universal paste gate and `Ctrl+Y` is the selected Reading binding.

## Decisions captured by this plan

1. The workspace contains an `e-dsh` executable package and an `e-tui` library package.
2. `e-tui` is a dedicated agent TUI library. It is not tied to the DSH wire protocol and may support other agent kernels through adapters.
3. Cargo dependency direction is `e-dsh -> e-tui`. Runtime data flows in both directions through `AgentEvent` and `UiAction` values.
4. The current Tokio runtime, bounded channels, event-driven frame scheduling, and short lock discipline remain in place during the refactor.
5. Rendering is composed as `Screen -> Pane -> Region -> Component`. The directory name `box` is not used because `box` is a Rust keyword.
6. The screen has a main pane and a Preview pane. The main pane contains the transcript, composer, and status regions.
7. The Preview pane is used in both normal mode and Reading View. Normal mode follows the newest semantic Block. Reading View follows the current Item or Block cursor.
8. Reading View replaces Copy Mode. It navigates semantic Blocks and Items rather than rendered rows.
9. Existing Markdown provenance, atomic copy sources, transcript layout cache, streaming tail splice, activity patches, and history anchors must survive the migration.
