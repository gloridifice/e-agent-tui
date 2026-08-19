# Migration plan

## Approach

The migration should preserve behavior while boundaries move. Crate extraction, state decomposition, rendering reorganization, the two-pane layout, and Reading View should not land as one rewrite.

The current client already has an acyclic production module graph, typed runtime effects, a single public transcript path, incremental rendering, and narrow infrastructure ports. The refactor should keep those properties and move them behind clearer package and state boundaries.

## Phase 0: baseline and change boundaries

Before moving code:

1. Record the current scoped test commands for runtime, projection, transcript layout, UI, setup, launcher, and wire conformance.
2. Capture the current timing baselines from `timing_snapshot` and `timing_frames`.
3. Keep `client/tests/architecture.rs` passing.
4. Confirm `cargo fmt --all`, scoped Clippy checks, and the protocol contract check pass.
5. Treat architecture extraction and the new two-pane behavior as separate review units, even if they belong to one larger plan.

No production behavior changes in this phase.

## Phase 1: create the workspace packages

Create:

```text
crates/e-dsh/
crates/e-tui/
```

Move the existing package to `crates/e-dsh` first and keep the binary name `dshe`:

```toml
[package]
name = "e-dsh"

[[bin]]
name = "dshe"
path = "src/main.rs"
```

Create an initially small `e-tui` library and add it as a path dependency of `e-dsh`.

Keep source installation behavior unchanged:

```powershell
cargo install --path crates/e-dsh --locked
```

The package split uses `cargo install --path crates/e-dsh --locked`. No `client` compatibility shim is retained: the source-install path is repository-local rather than a stable runtime API, and keeping a second manifest or package copy would create the duplicate ownership this migration forbids. README, AGENTS.md, and current architecture documentation change with the move.

Move `client/build.rs` to `crates/e-dsh/build.rs`. Update all paths to `bridge/protocol-contract.json` and `bridge/src` carefully because the relative depth changes.

Move UI assets to `crates/e-tui/assets`. Keep bridge embedding and generated wire constants in `e-dsh`.

Exit criteria:

- `cargo run` from the workspace root still launches `dshe`;
- setup embeds the same bridge files and digest;
- the executable has no visible UI change;
- current launcher, setup, wire, and bridge tests pass.

## Phase 2: split runtime contracts from infrastructure

Introduce the public `AgentEvent`, `InputEvent`, `UiAction`, and `UpdateResult` contracts in `e-tui`.

Adapt the current `RuntimeController` incrementally:

```text
current RuntimeInput       -> AgentEvent or InputEvent
current RuntimeEffect      -> UiAction
current ControllerAction   -> private TuiApp transition
```

Keep an adapter in `e-dsh` that maps DSH protocol values to normalized events and maps agent requests back to `ClientMessage` values.

Do not move DSH protocol enums into `e-tui` as a shortcut. That would make the new crate boundary nominal rather than useful.

Exit criteria:

- `e-tui` imports no DSH protocol modules;
- all I/O still executes after state guards are released;
- terminal, bridge, effect-completion, and queue-dispatch tests pass;
- protocol mismatch handling remains in the executable adapter.

## Phase 3: move the existing UI core

Move behavior-preserving modules into `e-tui`:

- display surfaces;
- transcript store and projection families after their inputs are normalized;
- input state and Input Page controllers;
- transcript layout and render cache;
- Markdown and Mermaid rendering;
- copy provenance;
- theme/config value schemas and profiling helpers.

Keep the current Ratatui renderer in `e-dsh` during this phase. Moving it before its consumed lifecycle state would either create an `e-tui -> e-dsh` dependency or require a large temporary host-facing render trait.

Split modules that contain both infrastructure and UI code. For example:

- config schema and resolved theme belong to `e-tui`;
- config paths and file persistence belong to `e-dsh`;
- clipboard selection logic belongs to `e-tui`;
- `arboard` calls belong to `e-dsh`;
- terminal event adaptation may belong to `e-tui`, while terminal lifecycle and the main loop stay under executable control.

Exit criteria:

- the screen is still the current single-column implementation owned temporarily by `e-dsh`;
- no copy, Markdown, history, or animation behavior changes;
- scoped projection, input, layout, copy, cache, and runtime tests pass after ownership changes.

## Phase 4: decompose `AppState`

Introduce `TuiApp` and move state one lifecycle at a time:

1. session and page state;
2. catalog and command state;
3. interaction state;
4. timeline and projection state;
5. render and provenance state.

Each extraction should have one owner and no mirror field left in the old facade. Temporary forwarding methods are acceptable, but temporary duplicate stores are not. Complete these lifecycle owners before moving the Ratatui renderer into `e-tui`; rendering must consume frontend-owned state directly rather than an `e-dsh` facade or compatibility trait.

Recommended target:

```rust
pub struct TuiApp {
    session: SessionModel,
    timeline: TimelineModel,
    interaction: InteractionModel,
    reading: Option<ReadingViewState>,
    preview: PreviewPaneState,
    catalogs: CatalogModel,
    render: RenderState,
}
```

Preserve current behavior for:

- snapshot replay;
- history prepend;
- surface replacement;
- tool correlation across page boundaries;
- retry enrichment;
- deferred `/new`;
- Question Input Page draft restoration;
- cache invalidation and targeted patching.

Exit criteria:

- production no longer uses an oversized compatibility facade for newly extracted lifecycles;
- no second transcript store exists;
- architecture tests cover the new state boundaries;
- scoped model, projection, runtime, and cache tests pass.

## Phase 5: extract and reorganize rendering without visual change

After Phase 4 owns every lifecycle consumed by rendering, first move the existing single-column Ratatui UI into `e-tui` unchanged. Run the captured TestBackend and release timing gate at that boundary, then reorganize rendering into:

```text
Screen -> Pane -> Region -> Component
```

Start with static function and module moves. Add a `UiComponent` trait only where a real substitution point exists.

Preserve the current page width, input, status, title, overlay, copy, and cursor behavior during this phase.

Important constraints:

- Components do not import Regions or Panes.
- Regions do not perform I/O.
- rendering does not mutate domain text;
- Markdown materialization and provenance stay cached;
- transcript and copy use the same width-aware layout;
- the terminal hardware cursor remains hidden.

Exit criteria:

- no visible layout change;
- UI spacing and color regression tests pass;
- architecture checks prove downward rendering dependencies;
- snapshot and continuous frame benchmarks do not regress materially.

## Phase 6: add the Preview pane in normal mode

Add `PreviewPaneState`, Preview rendering, and the two-pane Screen layout.

Normal mode uses:

```text
PreviewPolicy::FollowLatestBlock
```

Build the minimum semantic Block index needed to select the newest transcript Block. Every valid Block provides either a specialized Preview or a complete-source fallback.

Keep the composer and both status rows active in the main pane. The Preview pane uses the remaining width and full height.

Add inline Preview support first. Deferred resolution can wait until the selection and rendering path is stable.

Required regression coverage:

- the latest Block becomes the Preview target;
- streaming refreshes the same target;
- history prepend does not replace the target;
- an empty transcript produces the empty Preview state;
- changing target resets Preview scroll;
- narrow and wide layouts have asserted rectangles, line counts, colors, and content;
- transcript rewrap keeps copy provenance correct.

Exit criteria:

- normal interaction is unchanged apart from the planned two-pane layout;
- the Preview pane always reflects the newest Block;
- render cache work remains bounded.

## Phase 7: build `ReadingDocument` and `ReadingLayout`

Build the full semantic model alongside the old Copy Mode:

- stable `BlockId` values;
- `ReadingBlock` taxonomy;
- complete copy payloads;
- `ReadingItem` annotations;
- Block row ranges;
- Item layout fragments;
- Preview references.

The old Copy Mode remains available in this phase. Both paths must use the same underlying provenance and wrapping data.

Add characterization tests for:

- paragraph, code, list row, Mermaid, tool, and reasoning Blocks;
- Markdown links as Items;
- file references as Items;
- wrapped Item fragments;
- stable IDs across resize and streaming;
- hidden reasoning exclusion;
- surface replacement and history prepend.

Exit criteria:

- ReadingDocument can be built for snapshots, live events, and history pages;
- its identities survive width changes;
- the old UI still behaves as before.

## Phase 8: add Reading View Block mode

Select the Reading View binding, using `Ctrl+V` as the initial candidate, and implement:

- rejection when there is no eligible Block;
- nearest-to-center initial selection;
- one Block cursor;
- `j` and `k` Block navigation;
- top-third and bottom-third page scrolling;
- Night hover background;
- Bark gutter rail;
- `y` copying the current Block;
- `Esc` exit;
- composer draft preservation;
- Preview policy switching between latest Block and Reading cursor.

Do not remove old Copy Mode until Reading View has all required Item and Preview behavior.

Test `Ctrl+V` delivery in Windows Terminal, ConHost, and supported Linux terminals. Verify that bracketed paste still arrives as `Event::Paste`; if any supported terminal intercepts the candidate, select and test one documented alternate before removing Copy Mode.

Required UI tests:

- rail placement does not change wrapping or content x-position;
- hover background covers the Block rows without polluting explicit inline backgrounds;
- the selected Block remains stable after resize;
- entering and exiting restores the input draft;
- new live Blocks do not steal the Reading cursor or Preview target.

## Phase 9: add Item mode and Preview resolution

Implement:

- `l` entry into Item mode;
- spatial `h`, `j`, `k`, and `l` navigation;
- cross-Block up and down boundary behavior;
- left-boundary exit to Block mode;
- `Esc` return to Block mode;
- Item-local highlight;
- Item Preview precedence;
- inline Link, Diff, Lines, SearchResult, Command, Path, Markdown, and PlainText previews.

Then add deferred resolution:

- `UiAction::ResolvePreview`;
- request IDs and revisions;
- adapter-side async work;
- `PreviewResolved` events;
- result caching;
- stale-result protection;
- loading and error views.

Required tests:

- every directional boundary rule;
- multiline and wrapped Items;
- an Item always belongs to the selected Block;
- a late result never overwrites a newer target;
- cached results can be reused after moving away and back;
- normal mode and Reading View share the same Preview cache;
- no resolver holds a UI lock while awaiting.

## Phase 10: remove Copy Mode

Once Reading View covers the required copy behavior:

- change the public command from `Ctrl+B` to the selected Reading View binding;
- remove row selection and selection anchor state;
- remove Copy Mode overlays and range navigation;
- remove caches that serve only old row selection;
- keep source provenance, atomic unit identity, and complete copy payloads;
- rename any surviving copy-specific types to semantic reading or provenance names.

Do not remove atomic copy semantics for tables, code, or Mermaid. Reading View `y` copies the complete current Block.

Update help text, key references, and UI tests in the same change.

## Phase 11: final boundary cleanup

After behavior is stable:

- consider letting the main loop own `TuiApp` directly and removing `Arc<Mutex<_>>`;
- keep the mutex if ownership simplification does not justify its migration cost;
- remove temporary forwarding facades and adapters;
- run the full architecture scanner across both crates;
- update install paths and source-install documentation;
- archive this plan or mark completed sections explicitly.

Direct state ownership is an optional cleanup. It must not delay the crate split or Reading View.

## Current-to-target module map

| Current path | Target owner | Target path or role |
| --- | --- | --- |
| `client/src/main.rs` | `e-dsh` | thin entry point and runtime composition |
| `client/src/bridge_io.rs` | `e-dsh` | `bridge/io.rs` |
| `client/src/protocol.rs` and `protocol/` | `e-dsh` | `bridge/protocol.rs` and typed DSH parsing |
| `client/src/launcher.rs` | `e-dsh` | launcher infrastructure |
| `client/src/setup.rs` | `e-dsh` | setup and embedded bridge installation |
| `client/src/dsh_env.rs` | `e-dsh` | DSH profile environment |
| `client/src/runtime.rs` | split | `e-tui` updates, `e-dsh` orchestration and effects |
| `client/src/runtime_ports.rs` | split | executable effect ports and frontend event adapter |
| `client/src/config.rs` | split | schema in `e-tui`, storage in `e-dsh` |
| `client/src/theme.rs` | mostly `e-tui` | theme schema, parsing, and render values |
| `client/src/model.rs` | `e-tui` | decomposed model modules |
| `client/src/projection/` | `e-tui` | projection over normalized timeline events |
| `client/src/display.rs` | `e-tui` | public display surfaces and reading annotations |
| `client/src/input.rs` | `e-tui` | composer state and key behavior |
| `client/src/input_page.rs` | `e-tui` | Input Page session controller |
| `client/src/ui/` | `e-tui` | Regions, Panes, and Screen |
| `client/src/render.rs` | `e-tui` | Markdown and presentation Components |
| `client/src/transcript_layout.rs` | `e-tui` | shared width-aware layout kernel |
| `client/src/cache.rs` | `e-tui` | transcript and Preview caches |
| `client/src/copy.rs` | `e-tui` | provenance first, then Reading View copy behavior |
| `client/src/terminal_runtime.rs` | `e-dsh` or split | one terminal lifecycle owner; frontend rendering stays in `e-tui` |
| `client/src/profile.rs` | shared through `e-tui` API | feature-forwarded profiling helpers |
| `client/build.rs` | `e-dsh` | wire generation and bridge embedding |
| `client/assets/` | `e-tui` | default config and themes |

## Test strategy

### Architecture tests

Adapt `client/tests/architecture.rs` to scan both crates and assert:

- no SCC in production modules;
- no `e-tui -> e-dsh` dependency;
- no DSH wire imports in `e-tui`;
- no upward render dependency;
- one production transcript path;
- one config schema and default source;
- Preview background tasks return events instead of mutating state.

### State transition tests

Test `TuiApp::update` and input routing without a terminal or WebSocket:

- normalized event projection;
- session switching;
- deferred new session;
- queue dispatch;
- Reading View state invariants;
- Preview policy changes;
- async completion races.

### UI regression tests

Rendering and spacing changes require ratatui `TestBackend` assertions. Cover:

- normal 60/40 layout;
- Reading View layout;
- narrow layout policy;
- status and composer rows;
- Preview empty, loading, ready, and error states;
- Block rail and background;
- Item highlight;
- diff, lines, command, link, and fallback previews;
- cached line counts, colors, and visible content;
- cursor and input draft restoration.

### Performance tests

Preserve and extend current performance gates:

- startup snapshot timing;
- continuous streaming and animation frames;
- no full transcript rebuild per event;
- bounded visible row materialization;
- Preview target changes do not rebuild the transcript;
- normal latest-Block updates patch only affected Preview and transcript ranges;
- Reading cursor movement does not reparse Markdown;
- deferred Preview completion triggers one scheduled draw.

### Protocol and bridge tests

Any wire change still requires:

```powershell
cd bridge
npm test
node tools/sync-protocol-contract.mjs --check
```

After DSH compatibility changes, run the deployed-copy upgrade gate documented in [../bridge.md](../bridge.md).

The normalized `AgentEvent` contract is internal to the Rust workspace and does not automatically require a wire protocol bump. A bump is required only when the bridge messages change.

## Documentation work

During implementation, keep future and current behavior separate. Update current documentation only when a phase lands.

When the crate split lands, update:

- root workspace commands in `AGENTS.md`;
- [the root README](../../README.md) only if user-facing install commands change;
- [../client.md](../client.md) for the new crate and runtime boundaries;
- [../design.md](../design.md) for the two-pane layout decision;
- [the documentation index](../README.md).

When Preview and Reading View land, update:

- help overlay text;
- keybinding quick reference;
- copy and Reading View semantics;
- layout diagrams;
- config fields and defaults;
- TestBackend line-number assumptions.

All project documentation remains in English.

## Main risks

### Big-bang migration

Moving crates, state, rendering, and interaction together would make regressions hard to locate. The phase order keeps behavior-preserving work ahead of visual and input changes.

### Kernel abstraction that leaks DSH

Raw event types or tool JSON in `e-tui` would prevent reuse. The adapter contract and architecture tests must catch this early.

### Duplicate transcript models

A second Reading View message store would drift from rendering and history. `ReadingDocument` is a semantic index over the same timeline, not another transcript.

### Identity tied to rendered rows

Block and Item IDs must survive wrapping. Layout rows are derived data.

### Preview race conditions

Deferred results need request IDs, revisions, and target checks. A late result may populate cache but cannot replace the current target.

### Render cache regressions

The 60/40 layout changes transcript width and causes legitimate rewrap. It must not turn every stream chunk or cursor move into a full rebuild.

### Input binding compatibility

Some terminals may intercept `Ctrl+V`. The candidate needs platform tests before old Copy Mode is removed; if it fails, one documented alternate must become the selected binding everywhere.

### Documentation drift

This directory describes planned behavior. Current architecture documents should change phase by phase so they never claim unimplemented behavior is available.
