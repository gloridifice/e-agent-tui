# Target Rust architecture

## Status and scope

This document defines the target architecture for the Rust side of `e`. It covers crate ownership, the agent kernel boundary, state transitions, rendering composition, screen layout, and the runtime model.

The Node.js bridge remains a separate package under `bridge/`. The first executable adapter is DSH-specific, but `e-tui` must not depend on DSH wire messages or raw DSH tool names.

## Workspace shape

```text
crates/
├── e-dsh/                  # package e-dsh, binary dshe
└── e-tui/                  # library package e-tui
```

The workspace dependency graph has one Cargo edge:

```text
e-dsh -> e-tui
```

Runtime values travel in both directions:

```text
e-dsh --AgentEvent--> e-tui
e-dsh <--UiAction---- e-tui
```

This is not a reverse crate dependency. `AgentEvent` and `UiAction` are public contracts owned by `e-tui`; adapters construct and consume those values.

A future kernel executable can reuse the library without importing DSH code:

```text
DSH kernel   -> e-dsh adapter   -> e-tui
Kernel A     -> adapter A       -> e-tui
Kernel B     -> adapter B       -> e-tui
```

## Package responsibilities

### `e-dsh`

`e-dsh` is the imperative shell and DSH adapter. It owns:

- the `dshe` CLI and process entry point;
- the Tokio runtime and top-level `select!` loop;
- DSH launcher, setup, profile, and embedded bridge installation;
- WebSocket connection, wire encoding, wire decoding, and protocol checks;
- conversion from DSH frames to normalized `AgentEvent` values;
- conversion from `UiAction` values to DSH client messages;
- config and state-file persistence;
- clipboard access;
- process and filesystem effects;
- frame, content, and animation scheduling;
- async Preview resolution;
- terminal setup and restoration orchestration;
- the build script that reads `bridge/protocol-contract.json` and embeds the bridge runtime.

The package should keep `src/lib.rs` as well as `src/main.rs` so launcher, setup, protocol, and effect execution can be tested without spawning the binary.

Suggested structure:

```text
crates/e-dsh/
├── Cargo.toml
├── build.rs
└── src/
    ├── main.rs
    ├── lib.rs
    ├── runtime.rs
    ├── effect_executor.rs
    ├── bridge/
    │   ├── mod.rs
    │   ├── io.rs
    │   ├── protocol.rs
    │   └── adapter.rs
    ├── launcher.rs
    ├── setup.rs
    ├── dsh_env.rs
    ├── config_store.rs
    └── clipboard.rs
```

### `e-tui`

`e-tui` is a dedicated frontend library for interactive agent sessions. It owns:

- normalized agent events and UI actions;
- the synchronous UI state machine;
- transcript projection and public display surfaces;
- session and catalog presentation state;
- terminal input state and focus routing;
- Reading View and semantic navigation;
- the unified Preview pane and its cache state;
- Markdown, text, diff, diagram, and status presentation;
- copy provenance and copy payloads;
- transcript layout and render caches;
- ratatui screen composition;
- theme and config schemas used by the UI;
- the crossterm terminal event adapter, while the executable drives its future.

It does not own:

- WebSocket types;
- DSH `ServerMessage` or `ClientMessage` types;
- raw DSH event names;
- DSH profile and setup behavior;
- filesystem persistence;
- clipboard access;
- a Tokio runtime;
- background tasks that mutate UI state directly.

Suggested top-level structure:

```text
crates/e-tui/
├── Cargo.toml
├── assets/
│   ├── default_config.toml
│   └── themes/
└── src/
    ├── lib.rs
    ├── agent/
    ├── action/
    ├── app/
    ├── event/
    ├── input/
    ├── model/
    ├── preview/
    ├── projection/
    ├── reading/
    └── render/
```

## Kernel-neutral contract

### Events are facts

`AgentEvent` describes facts already observed by an adapter or completed effect. A top-level event groups related event families rather than growing into one flat enum.

```rust
pub enum AgentEvent {
    Session(SessionEvent),
    Timeline(TimelineEvent),
    Catalog(CatalogEvent),
    Interaction(InteractionEvent),
    Preview(PreviewEvent),
    EffectCompleted(EffectResult),
    Deadline(DeadlineEvent),
}
```

Terminal input can use a separate `InputEvent` because it is produced inside the frontend boundary:

```rust
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
}
```

### Actions are requests

`UiAction` describes work that the executable or kernel adapter must perform after the UI state guard has been released.

```rust
pub enum UiAction {
    Agent(AgentRequest),
    ResolvePreview(PreviewRequest),
    PersistConfig(Config),
    PersistSessionId(String),
    WriteClipboard(String),
    RequestDraw(DrawPriority),
    Quit,
    Fatal(String),
}
```

Every action owns the payload required by its executor. The executor must not borrow `TuiApp` while it performs I/O or awaits a future.

### Kernel capability normalization

Adapters map raw tool names and arguments to a small presentation contract:

```rust
pub enum ToolCapability {
    Read,
    View,
    Edit,
    Replace,
    Search,
    Command,
    Create,
    Generic,
}
```

For example, DSH `str_replace_editor` variants and a future kernel's patch operation may both map to `Edit` or `Replace`. `e-tui` renders the normalized capability and any adapter-provided reading annotations. It does not parse raw tool JSON.

The normalized tool model carries enough information to build transcript activity and Reading View metadata:

```rust
pub struct ToolActivity {
    pub id: ActivityId,
    pub capability: ToolCapability,
    pub label: String,
    pub summary: String,
    pub state: ActivityState,
    pub preview: Option<PreviewRef>,
    pub items: Vec<ToolItem>,
}
```

A namespaced `Custom` form may be added for capabilities that cannot use a standard presentation without losing useful information.

## State ownership

The current `AppState` facade should be decomposed by lifecycle, not by file size alone.

```rust
pub struct TuiApp {
    pub session: SessionModel,
    pub timeline: TimelineModel,
    pub interaction: InteractionModel,
    pub reading: Option<ReadingViewState>,
    pub preview: PreviewPaneState,
    pub catalogs: CatalogModel,
    pub render: RenderState,
}
```

### `SessionModel`

Owns attached session identity, title, workspace, provider, model, mode, status, token usage, deferred new-session state, and history page state.

### `TimelineModel`

Owns `TranscriptStore`, event projection correlations, public display surfaces, copy source identity, and semantic reading annotations.

### `InteractionModel`

Owns composer state, scroll state, focus, Input Page state, approval state, help state, queue state, and transient notices.

### `ReadingViewState`

Owns the Block cursor, optional Item cursor, and Reading View navigation state. The full contract is defined in [reading-view.md](reading-view.md).

### `PreviewPaneState`

Owns the active Preview policy, target, loading state, resolved value, scroll position, and cache keys. It does not duplicate transcript or tool data.

### `RenderState`

Owns width-dependent layout registries, transcript render cache, Preview render cache, animation transitions, copy provenance, and frame-local metrics.

A Region may own local interaction state, such as Preview scroll or a page viewport. It must not copy session or transcript records into a second domain store.

## Update and effect flow

`e-tui` exposes synchronous updates:

```rust
impl TuiApp {
    pub fn update(&mut self, event: AgentEvent) -> UpdateResult;
    pub fn handle_input(&mut self, event: InputEvent) -> UpdateResult;
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
    ) -> RenderOutput;
}
```

```rust
pub struct UpdateResult {
    pub actions: Vec<UiAction>,
    pub dirty: DirtyState,
    pub next_deadline: Option<Instant>,
}
```

The executable follows one sequence for every event:

```text
receive event
  lock TuiApp if the migration still uses a shared mutex
  apply synchronous update
  collect complete UiAction values
  release the lock
  execute actions and await I/O
  send completion facts back as AgentEvent values
  draw when the scheduler deadline is due
```

No Region or Component sends to the kernel directly.

## Rendering architecture

Rendering uses four levels:

```text
Screen -> Pane -> Region -> Component
```

The compile-time dependency direction follows the same order:

```text
render/mod.rs
  -> render/pane/
       -> render/region/
            -> render/component/
```

### Component

A Component is a reusable rendering primitive. Examples include:

- working indicator;
- card shell;
- plain text;
- status text;
- diff presentation;
- Markdown block materialization.

A rendering primitive should implement ratatui `Widget` when that is sufficient. A common trait is useful only when several primitives share an actual substitution point.

If a trait is used, rendering receives a `Rect`, not only a `Size`, because a nested component needs both origin and dimensions:

```rust
pub trait UiComponent {
    fn render(&self, frame: &mut Frame<'_>, area: Rect);
}
```

A card wrapper is better expressed as a shell that draws the border and background, then returns its inner `Rect`. This avoids boxed child widgets.

### Region

A Region binds a view model to rendering primitives and handles region-specific interaction. Planned Regions are:

- transcript;
- composer;
- status;
- Preview;
- Input Page shell and page bodies.

Event updates and drawing remain separate. A Region does not change domain text while it renders.

### Pane

The main pane arranges transcript, composer, and status Regions. The Preview pane renders `PreviewPaneState`.

### Screen

The Screen computes responsive rectangles, assembles both panes, and draws overlays that belong above the complete layout.

Suggested rendering structure:

```text
render/
├── mod.rs
├── layout.rs
├── component/
│   ├── mod.rs
│   ├── working_indicator.rs
│   ├── card.rs
│   ├── markdown.rs
│   ├── text.rs
│   ├── diff.rs
│   └── status_text.rs
├── region/
│   ├── mod.rs
│   ├── transcript/
│   ├── composer/
│   │   ├── bar.rs
│   │   ├── text_box.rs
│   │   └── page/
│   ├── status/
│   └── preview/
└── pane/
    ├── mod.rs
    ├── main.rs
    └── preview.rs
```

The directory is named `region`, not `box`, because `box` is a Rust keyword.

## Two-pane screen layout

### Normal mode

Normal mode keeps the composer active. The Preview pane follows the newest semantic Block.

```text
+--------------------------------+----------------------+
| Transcript                     | Preview              |
|                                | latest Block         |
+--------------------------------+                      |
| Composer                       |                      |
+--------------------------------+                      |
| Status line                    |                      |
| Session title and workspace    |                      |
+--------------------------------+----------------------+
```

The left pane contains all three main Regions. The Preview pane uses the full available height.

### Reading View

Reading View replaces the composer with a compact navigation strip. The Preview pane follows the current Item or Block cursor.

```text
+--------------------------------+----------------------+
| Transcript                     | Preview              |
| Block and Item cursor          | selected Item/Block  |
+-------------------------------------------------------+
| READING: navigation and copy keys                     |
+-------------------------------------------------------+
| Status line                                           |
| Session title and workspace                           |
+-------------------------------------------------------+
```

The implementation may keep the status rows inside the left pane if that produces a cleaner ratatui layout, but both modes must present the same information and preserve the input draft across Reading View entry and exit.

### Width calculation

For usable screen width `W`:

```text
main_width = min(floor(0.6 * W), width_config)
preview_width = W - main_width
```

The transcript page is left-aligned inside the main pane in Reading View. The Preview pane receives all remaining columns.

The implemented layout stores the message-pane share as `message_pane_percent` (25.00%–100.00%, default 60.00%) and derives columns from the current terminal width. Preview remains beside the message pane only when its raw rectangle has at least 19 columns: a separator column, a one-column gap, 16 usable content columns, and a one-column right margin. Otherwise the normal view keeps a right-margin separator grip and the full-screen Preview toggle remains available. Main content uses one ordinary horizontal edge column, with collapsed grip geometry reserved in Main-only mode. This is a layout policy, not a change to Preview selection semantics.

## Markdown and provenance pipeline

Markdown and diff rendering are not simple frame-only widgets. The current copy and performance contracts require a two-stage pipeline:

```text
source
  -> parser and semantic block builder
  -> RenderedBlock with styled lines and provenance
  -> width-dependent layout cache
  -> visible row materialization
  -> frame
```

A rendered block retains:

- styled lines;
- source ranges or complete copy payloads;
- atomic copy identity for code, table, and diagram blocks;
- stable Block and Item identities;
- Item layout fragments for spatial navigation.

Markdown must not be reparsed on every frame. Streaming updates keep stable unit and Block identities while refreshing only the affected tail.

## Runtime and thread model

The current runtime model remains in place during the refactor. Splitting crates does not create a new thread boundary.

```text
dshe process
└── Tokio runtime
    ├── WebSocket reader task
    │   └── bounded AgentEvent channel
    ├── WebSocket writer task
    │   └── bounded kernel command channel
    ├── optional Preview resolver tasks
    │   └── PreviewResolved events
    └── main controller loop
        ├── terminal EventStream
        ├── bounded inbound batch
        ├── frame deadline
        ├── animation deadline
        ├── TuiApp update
        ├── UiAction execution
        └── frame draw
```

`e-tui` does not create another Tokio runtime. Preview resolver tasks run in the executable or adapter and return results through events.

### Lock discipline

The first migration stages may retain `Arc<std::sync::Mutex<TuiApp>>` to limit risk. The existing rule remains mandatory:

```text
lock
  update state
  create actions with owned payloads
unlock
await effects
```

Rust 2021 scrutinee temporaries must not hold a state guard across a branch that re-locks or awaits. The executable should keep `#![deny(clippy::significant_drop_in_scrutinee)]` and the current lock-release tests.

Once the crate boundary is stable, the main loop may own `TuiApp` directly because WebSocket tasks communicate through channels. Removing the mutex is optional and is not part of the initial extraction.

### Existing runtime constraints to preserve

- Terminal input uses `EventStream` and wakes the loop directly.
- The loop does not use a fixed ticker.
- Inbound work is bounded by message count and elapsed time.
- Interaction, content, and animation deadlines remain separate.
- WebSocket channels remain bounded.
- Terminal lifecycle has one owner.
- The UI never performs I/O or awaits while holding a state guard.
- Streaming uses tail splice rather than a full cache rebuild.
- Animation uses targeted patches.
- Each frame materializes only the visible transcript and Preview rows.
- A Preview completion requests a draw through the existing scheduler.

## Dependencies and features

`e-tui` should contain UI dependencies such as ratatui, crossterm, Unicode handling, Markdown parsing, Mermaid rendering, serde for public value types, and TOML for the config schema.

`e-dsh` should contain tokio-tungstenite, URL handling, filesystem paths, directories, clipboard integration, launcher process control, and DSH protocol serialization.

If terminal input remains in `e-tui`, the library may expose an async event source without owning an executor. `e-dsh` supplies the Tokio runtime.

The `tracy` feature should be forwarded from `e-dsh` to `e-tui` so both crates use the same profiling build. The existing no-op behavior when no Tracy client is active must remain.

## Architecture guards

The current dependency scanner and SCC test should be adapted to both crates. The target checks are:

- no production SCC inside either crate;
- no dependency from `e-tui` to `e-dsh`;
- no DSH protocol imports inside `e-tui`;
- transcript layout remains a presentation leaf;
- Components do not depend on Regions, Panes, or the Screen;
- Preview resolution performs no direct state mutation from a background task;
- production transcript storage still uses one public display path.
