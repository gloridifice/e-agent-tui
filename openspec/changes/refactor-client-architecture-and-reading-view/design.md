## Context

The current Rust client is one package under `client/`. It already has valuable invariants: typed runtime effects, an acyclic production module graph, one transcript projection path, bounded channels and inbound batches, event-driven frame scheduling, short state-lock lifetimes, cached Markdown/provenance, streaming tail splice, targeted activity patches, and history anchors.

The planned Reading View and continuously visible Preview sidebar cut across protocol adaptation, state ownership, transcript semantics, layout, input routing, copying, and asynchronous effects. Adding them directly to the current facade would increase coupling and risk a second transcript model. The refactor therefore establishes package and rendering boundaries before introducing new visual behavior.

The current main column is the visual baseline. Its transcript surfaces, cards, Markdown, composer, status rows, spacing, colors, and copy provenance remain the target appearance after extraction and after it becomes the main pane. The Preview sidebar is new visual territory: it may reuse current primitives or add sidebar-specific components, but it must use the active theme and form one coherent screen rather than restyling the main pane around it.

## Goals / Non-Goals

**Goals:**

- Extract a reusable, kernel-neutral `e-tui` library behind owned event and action values.
- Keep DSH wire types, setup, persistence, process control, I/O, and async orchestration in `e-dsh`.
- Decompose state by lifecycle and rendering by `Screen -> Pane -> Region -> Component` without a visual rewrite of the main column.
- Add one Preview state model and cache shared by normal mode and Reading View.
- Replace rendered-row Copy Mode with stable semantic Block and Item navigation while retaining complete-source copy provenance.
- Preserve runtime safety, projection semantics, history behavior, incremental rendering, and frame-performance redlines during every phase.
- Land the work in independently reviewable, behavior-preserving and behavior-adding stages.

**Non-Goals:**

- Changing the Node.js bridge or WebSocket contract unless implementation proves that required Preview data cannot be derived in the adapter.
- Creating a new Tokio runtime or allowing background tasks to mutate UI state directly.
- Replacing the current display-surface model, Markdown renderer, theme language, or main-pane visual identity.
- Building a second transcript store for Reading View.
- Supporting multi-Block selection or dedicated Preview keyboard focus in the first Reading View release.
- Removing `Arc<Mutex<TuiApp>>` during the initial extraction; direct main-loop ownership is optional cleanup.

## Decisions

### 1. Split the workspace into `e-dsh` and `e-tui`

`crates/e-dsh` owns the `dshe` binary, Tokio controller loop, DSH bridge adapter, protocol, setup, launcher, persistence, clipboard and terminal effects. `crates/e-tui` owns normalized contracts, synchronous UI state transitions, input adaptation, transcript projection over normalized events, Reading View, Preview presentation state, rendering, theme/config schemas, and render caches. Cargo dependency direction is only `e-dsh -> e-tui`.

This boundary is preferred over feature-gating DSH code in one crate because a feature flag would not prevent UI code from importing protocol types. It is preferred over a callback-heavy frontend API because owned values are easier to test and preserve lock discipline.

### 2. Use facts in and owned requests out

`e-tui` accepts `AgentEvent` facts and `InputEvent` values through synchronous update methods. It returns `UpdateResult { actions, dirty, next_deadline }`; each `UiAction` owns everything the executor needs. The UI `Config` and theme value schemas move into `e-tui` before this public action contract is introduced, while paths and persistence remain in `e-dsh`. DSH frames are normalized in `e-dsh`, including mapping raw tools to `ToolCapability` and adapter-provided Preview annotations.

During contract extraction, the existing `e-dsh` runtime controller may temporarily own legacy UI state while consuming normalized `AgentEvent` values and returning `UiAction` values. This is a forwarding boundary, not a second state store. The synchronous update owner moves into `e-tui` with the UI core. At every stage, the executable releases any UI state guard before executing or awaiting an action, then returns completion facts as events.

### 3. Decompose state by lifecycle, not file size

`TuiApp` owns `SessionModel`, `TimelineModel`, `InteractionModel`, optional `ReadingViewState`, `PreviewPaneState`, `CatalogModel`, and `RenderState`. Extraction proceeds one owner at a time; forwarding methods are temporary, but mirror stores are forbidden.

During UI-core extraction, `TimelineModel` moves first because display projection and transcript storage already form one lifecycle. The legacy `e-dsh::AppState` may forward timeline operations to that owner until the remaining lifecycle models move, but it must not retain a second transcript or projection-family state. The remaining lifecycle owners are extracted before the Ratatui UI crosses the package boundary, so `e-tui` rendering consumes frontend-owned models directly rather than depending on an `e-dsh` facade or a temporary host-facing render trait.

`ReadingDocument` is a semantic index over the same timeline and provenance data. `PreviewPaneState` stores target, policy, resolution state and scroll—not a duplicate of transcript or tool records.

### 4. Enforce `Screen -> Pane -> Region -> Component`

The screen computes responsive rectangles and overlays. Panes arrange regions. Regions bind view models and local interaction to rendering primitives. Components are reusable leaves and never import regions, panes, or the screen. A common component trait is introduced only where there is a true substitution point; otherwise ordinary Ratatui widgets and functions remain simpler.

Rendering modules are moved without visual change before the two-pane screen lands. Architecture tests enforce downward imports.

### 5. Preserve the main pane; design the Preview pane independently

The main pane continues to render the existing transcript, composer/Input Page, status line, and session/workspace title with the current surface components, theme tokens, spacing rules, and cursor behavior. Width changes may legitimately rewrap content, but do not justify new borders, card treatments, typography, or color semantics in the main pane.

The Preview pane may reuse Markdown, text, diff, card-shell, status-text, and working-indicator components or introduce Preview-specific components where its information density requires them. New components consume existing theme tokens by default; any new sidebar token must have defaults for every bundled theme. TestBackend characterization tests capture the current main-column appearance before rendering moves and assert it again at the pane's effective width.

This is preferred over redesigning both panes together because it isolates visual regressions and keeps the refactor reviewable.

### 6. Use one responsive two-pane screen policy

At sufficient width, usable width `W` is split using `main_width = min(floor(0.6 * W), width_config)` and Preview receives the remainder. Preview receives full height; the main pane owns transcript, composer/navigation strip, and status regions.

The Preview pane must retain at least 32 columns. Before Phase 6 implementation starts, characterization tests will establish the minimum safe main-pane width and an unclaimed full-screen Preview toggle binding. Below the combined minimum, the default remains main-only and the toggle presents Preview full-screen. This avoids making the current main UI unusable on narrow terminals.

### 7. Separate semantic identity from rendered geometry

`ReadingDocument` contains stable `ReadingBlock` and `ReadingItem` identities, kinds, complete copy payloads, and Preview references. `ReadingLayout` derives Block row ranges, gutter rails, and Item fragments for one effective transcript width from the same width-aware layout/provenance pipeline used by rendering.

IDs derive from stable display ownership and semantic unit identity, not row numbers. Resizing, wrapping, streaming growth, tool settlement, theme rematerialization, animation, and history prepend preserve IDs where semantic ownership remains unchanged.

### 8. Share one Preview policy, resolver and cache

Normal mode follows the newest Block. Reading View follows the current Item, falling back to its Block. Every Block has either a specialized Preview or a complete-source fallback.

Inline Preview values render immediately. Deferred references emit `ResolvePreview` with key, revision and request ID. Results are cached, but only a result matching the current key, revision and request may replace the visible state. Target changes reset Preview scroll. Background work occurs only in `e-dsh` and returns a completion event.

A single cache is preferred over mode-specific caches because moving between normal and Reading modes should reuse resolved data and obey identical stale-result rules.

### 9. Replace Copy Mode only after semantic parity

Reading View is added alongside old Copy Mode. The selected Reading View binding uses `Ctrl+V` as its default candidate; it selects the eligible Block nearest the viewport center, preserves the composer draft, and changes Preview policy. Block mode navigates Blocks; Item mode uses spatial geometry. `y` always copies the complete current Block, including when an Item is selected.

Old row selection, anchors, overlays, and `Ctrl+B` are removed only after Block/Item navigation, atomic copy sources, resize stability, history behavior, and terminal delivery of the selected Reading View binding pass the required gates. If supported terminals cannot reliably deliver `Ctrl+V`, one documented alternate is selected and used consistently before removal.

### 10. Keep the runtime and performance model unchanged

The Tokio runtime, EventStream wakeups, bounded channels, bounded inbound batches, independent interaction/content/animation deadlines, terminal lifecycle owner, tail splice, targeted patches, visible-row materialization, and optional Tracy integration remain. Preview completion requests one scheduled draw and does not rebuild the transcript. Reading cursor movement does not reparse Markdown.

## Risks / Trade-offs

- **[Big-bang regressions]** Moving packages, state, rendering, and interaction together would obscure failures. → Land the migration phases sequentially with an explicit test and benchmark gate per phase.
- **[Nominal kernel abstraction]** DSH tool JSON or wire enums could leak into `e-tui`. → Normalize in the adapter and enforce forbidden-import architecture tests.
- **[Main-pane visual drift]** Module moves or shared sidebar styling could subtly alter current spacing and colors. → Capture TestBackend baselines first; preserve existing main components and review sidebar styling separately.
- **[Duplicate transcript truth]** Reading View could drift from projection/history. → Build `ReadingDocument` as a derived semantic index using stable display/provenance identities; forbid a second public transcript store.
- **[Resize and streaming identity loss]** Row-derived IDs would move the cursor unexpectedly. → Keep IDs width-independent and rebuild only geometry after layout changes.
- **[Preview races]** A late async result could overwrite a newer target. → Match request ID, key and revision before display; cache stale results without displaying them.
- **[Narrow-terminal usability]** A permanent 60/40 split can starve both panes. → Enforce minimum widths and fall back to main-only/full-screen Preview.
- **[`Ctrl+V` interception]** Some terminals reserve paste before Crossterm. → Test Windows Terminal, ConHost and supported Linux terminals before deleting Copy Mode.
- **[Install disruption]** Moving `client` changes source-install commands and build-script relative paths. → Keep root default-member behavior, verify bridge digest/setup, and document a temporary compatibility path if needed.
- **[Performance regression]** Two layouts and Preview rendering could trigger whole-transcript work. → Share layout/provenance, isolate Preview cache invalidation, and retain bounded-work and release benchmark gates.

## Migration Plan

1. Record scoped test commands, TestBackend main-column baselines, architecture results, protocol check, and release timing baselines.
2. Move the existing package to `crates/e-dsh`, create a minimal `crates/e-tui`, repair build-script/asset paths, and verify no visible behavior change.
3. Move UI config/theme value schemas (but not persistence) into `e-tui`, then introduce normalized contracts and DSH adapters; the existing controller remains a transitional state owner and preserves effect execution after guard release.
4. Move projection, input, layout, copy provenance, caches, and other non-rendering frontend owners into `e-tui` without changing the single-column screen.
5. Decompose `AppState` into lifecycle owners without duplicate stores while the existing `e-dsh` renderer temporarily consumes their public view state.
6. Move the existing single-column UI into `e-tui`, pass the ownership-move characterization/performance gate, then reorganize rendering into Screen, Pane, Region, and Component modules.
7. Finalize narrow-layout thresholds/toggle binding, then add the normal-mode Preview pane and inline fallback previews.
8. Build width-independent `ReadingDocument` and width-dependent `ReadingLayout` alongside Copy Mode.
9. Add Block-mode Reading View and cursor-driven Preview, then Item navigation and deferred Preview resolution.
10. Verify terminal bindings and atomic copy parity, remove Copy Mode, update help/config/current documentation, and run final architecture/performance gates.
11. Optionally remove the shared UI mutex and temporary forwarding facades after the package boundary is stable.

Each phase must leave the workspace buildable and behaviorally coherent. Before the package move, rollback is a normal source revert. During the staged move, temporary forwarding modules may be retained for one phase; duplicate package copies or duplicate transcript stores are not valid rollback mechanisms.

## Open Questions

- What measured minimum main-pane width preserves the current composer, status, and transcript usability, and therefore determines the exact narrow fallback threshold?
- Which currently unclaimed key should toggle full-screen Preview on narrow terminals?
- Does every required DSH Preview map from existing normalized event data, or will a narrowly scoped bridge addition be required for one deferred resolver?
- Should direct main-loop ownership replace `Arc<Mutex<TuiApp>>` after extraction, or does the migration cost outweigh the simplification?
