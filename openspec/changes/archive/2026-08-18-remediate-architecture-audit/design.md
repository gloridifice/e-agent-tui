## Context

The project has two deployables: a Rust Ratatui client and a Node host-composition bridge. Their process boundary is intentionally narrow and the bridge already isolates most DSH service-locator knowledge behind `host.js`, but the Rust client has grown around shared concrete types rather than a dependency policy. Two strongly connected components now couple command discovery, input state, copy layout, rendering, Input Page control, login, and settings.

A second source of coupling is the unfinished display migration. `HostEvent` is typed and `EventProjector` exists, yet `AppState` still stores legacy `Msg` variants and `ui.rs` converts several variants into the four public display surfaces at render time. The main loop, host-event parser, compatibility reducer, and UI page renderers remain large coordination hotspots.

The change must preserve the project's existing performance and correctness constraints: no fixed ticker, bounded inbound work, tail-only streaming updates, range-only animation patches, shared copy/layout provenance, historical viewport anchoring, lock discipline, Windows process-tree shutdown, PowerShell 5.1 deployment compatibility, and backwards-compatible user configuration.

## Goals / Non-Goals

**Goals:**

- Make the production Rust module graph acyclic and enforce the allowed dependency direction in tests.
- Make the four display surfaces the only stored/rendered transcript representation.
- Reduce orchestration hotspots by moving event families, page rendering, and side effects behind explicit boundaries.
- Make runtime and launcher behavior deterministic under scripted terminal, bridge, clock, clipboard, process, and lock-store tests.
- Give wire protocol metadata and payload compatibility one machine-readable owner across Rust, Node, package metadata, fixtures, and documentation.
- Give each persisted configuration field one Rust schema declaration while retaining embedded defaults and partial user overlays.
- Remove unguarded duplication of DSH model-selection installation behavior.
- Preserve visible TUI behavior and performance semantics throughout the migration.

**Non-Goals:**

- Redesign the TUI, change keybindings, or add new user-facing commands.
- Change the semantic meaning of existing WebSocket frames or remove compatibility with older optional fields.
- Introduce a general plugin renderer ABI, ECS, or generic application framework.
- Replace Ratatui/Crossterm, Tokio, serde, or the DSH host-composition model.
- Restore unbounded event/tool payloads or relax frame limits.
- Re-enable historical vendoring under `client/vendor/`.

## Decisions

### 1. Enforce a directed client module graph with three leaf kernels

Create three low-level modules with no dependency on orchestration or concrete page/controller modules:

- `command_catalog`: built-in command declarations, integrated-command merging, ranking, and completion context. It may depend on protocol descriptor DTOs but not `InputState`, `AppState`, copy mode, pages, or async senders.
- `page_core`: focus graph, direction normalization, text editor, viewport, and generic `PageOutcome`/`PageEffect`. It does not import login, settings, model, theme, or resume page implementations.
- `transcript_layout`: width-aware display-row layout plus copy provenance. Both `ui` and `copy` consume its immutable result; `copy` no longer calls `ui`. 为支持 source-only `TranscriptBlock`，它可单向依赖 `display`、`render` 与只读 theme/config presentation leaf，但不得依赖 `AppState`、`ui`、`copy` 或事件 reducer。

The higher-level direction is:

```text
main/controller
  -> runtime_command -> command_catalog
  -> input           -> command_catalog
  -> input_page      -> page implementations -> page_core
  -> ui              -> transcript_layout
  -> copy            -> transcript_layout
  -> state/projection/render/protocol/config leaf services
```

An integration test under `client/tests/architecture.rs` will scan production `use crate::...` edges, assert forbidden reverse edges are absent, and run an SCC check. This lightweight in-repository check is preferred over adding an architecture-analysis dependency. The test will ignore `#[cfg(test)]` modules and document exceptional composition-root fan-out.

Alternative considered: retain Rust's legal module cycles and rely on reviewer discipline. Rejected because the current cycles already obscure ownership and make future regressions likely.

### 2. Store only public display surfaces in transcript state

Replace `AppState.msgs: Vec<Msg>` with a transcript store whose nodes are the public display contracts (`ActivityRow`, `TranscriptBlock`, `ContentCard`, and explicit composites where one event owns both activity and detail). Non-transcript state—approval, questions, todo, goal, mode, usage, title, queue—remains typed fields outside that store.

`EventProjector` becomes the sole HostEvent-to-display boundary. Event-family reducers (`assistant`, `tool/file`, `lifecycle`, `workflow/retry/command`, `surface`) emit typed mutations rather than legacy messages. Display identity, correlation indexes, surface ownership, shadowed sequence tracking, and pending cross-page halves remain in projection state.

`transcript_layout` consumes the transcript store and returns styled display rows plus provenance. Assistant Markdown remains source-only in `TranscriptBlock`; a layout-owned registry keyed by stable `DisplayId` materializes `RenderLine`s, assigns/reuses the complete unit-id range for each block, and retains atomic/raw-line provenance as cache sidecar state. `TranscriptBlock.unit` is only the block's primary unit and is not treated as the complete Markdown provenance model. The transcript store therefore continues to contain only public display surfaces and never stores Ratatui/render-specific lines. Theme/config/expanded changes explicitly invalidate registry entries, while streaming source growth rematerializes from the existing `unit_start` and marks only the tail dirty.

The render cache indexes stable `DisplayId`s; streaming mutates only the tail block, activity transitions mark only their ranges dirty, and structural replacements invalidate only the required structure. This avoids replacing the current incremental performance model with a simpler but slower rebuild model.

Alternative considered: keep `Msg` as private storage and only hide its adapters. Rejected because it preserves two vocabularies and lets cache/copy semantics diverge from the declared display model.

### 3. Turn the main loop into a reducer plus effect executor

Introduce a `RuntimeController` that receives typed inputs such as bridge frames, terminal keys/paste/mouse, animation deadlines, and frame deadlines. Handlers may first return internal `ControllerAction` values for state transitions such as opening/closing an Input Page; the controller consumes those actions under one scoped guard and never exposes them to the infrastructure executor. Only lock-external work becomes a `RuntimeEffect`, and every effect carries its complete payload: for example `Send(ClientMessage)`, `PersistConfig(Config)`, `PersistSessionId(String)`, `WriteClipboard(String)`, `RequestDraw(DrawPriority)`, `Quit`, and `Fatal(String)`.

The production runner in `main.rs` remains the Tokio composition root: it owns `select!`, channels, terminal setup/restoration, deadline scheduling, and effect execution through narrow transport/config/state-file/clipboard/draw ports. The executor does not receive the controller's mutable UI state and may not silently ignore an effect. `main.rs` must not contain page-, command-, copy-, or protocol-family business branches. Tests drive the controller directly with scripted events, so they do not require a real terminal or WebSocket.

The launcher receives a `LauncherPorts` boundary covering probe, spawn/terminate/reap, filesystem lock operations, and clock/sleep. Production functions remain thin wrappers around `std`; tests use an in-memory lock store and scripted processes. Platform-specific process-tree integration tests remain as a smaller final safety layer.

Alternative considered: use async traits for every dependency. Rejected because it adds object/lifetime complexity and a dependency; typed effects and narrow synchronous ports provide the required seams with less machinery.

### 4. Split parsers, reducers, and renderers by stable domain family

Keep public façades stable while moving implementation into submodules:

```text
protocol/
  messages.rs
  host_event/{content,assistant,tool,lifecycle,workflow}.rs
projection/
  surface.rs
  store.rs
  assistant.rs
  activity.rs
  workflow.rs
ui/
  transcript.rs
  input.rs
  accessories.rs
  pages/{settings,login,model,theme,resume}.rs
runtime/
  controller.rs
  effects.rs
  bridge_messages.rs
  terminal_events.rs
```

Closed matches over external wire enums remain valid; the goal is not polymorphism but cohesive family ownership. The façade modules re-export stable types so the migration can proceed incrementally. Because the current assistant/tool/lifecycle/workflow reducers still live in the legacy `model.rs::reduce_host_event` path, they SHALL NOT first be copied into temporary projection modules: `projection::surface` is extracted independently, while family modules are created during tasks 7.2–7.5 by translating each family directly into `TranscriptStore` mutations.

No universal line-count gate will be used. Instead, architecture tests enforce boundaries and focused tests prove each extracted family. This avoids gaming a metric while still eliminating the identified 300–600-line multi-responsibility routines.

### 5. Extend the contract additively and generate all derived protocol facts

Keep existing `clientMessages`, `serverMessages`, limits, and surface rosters for compatibility, and add a machine-readable message-shape section describing required and optional fields, primitive/container kinds, and referenced payload records. The format remains dependency-free JSON rather than introducing full JSON Schema tooling.

A single synchronization tool will:

1. validate the contract,
2. generate Rust constants/shape fixtures used by `build.rs`,
3. generate protocol documentation,
4. update `bridge/package.json.dshCompatibility.wireProtocol`, and
5. emit bounded Rust/Node conformance fixtures.

Rust serde enums and Node handlers may remain hand-written because they contain language-specific behavior, but tests must prove every contract message exists, accepted/produced fields conform, generated artifacts are current, and package metadata equals the canonical version. The immediate drift from 3 to 4 is fixed in the first protocol task.

Alternative considered: generate all Rust and JavaScript protocol code. Rejected for this change because custom serde defaults, bounded event parsing, and handler behavior would make the generator larger than the protocol benefit.

### 6. Merge TOML values before one strict Config deserialization

Derive `Deserialize` directly for the persisted portion of `Config`; the runtime-only resolved theme remains skipped/defaulted. Parse the embedded default TOML to `toml::Value`, parse the user file to another value, filter user keys against the embedded schema for legacy tolerance, recursively overlay accepted values, then deserialize once into `Config` with unknown fields denied.

This removes `CompleteConfig`, `PartialConfig`, `into_config`, and the parallel `apply` list. The embedded default remains the only owner of default values. `settings::SettingDef` remains only for fields intentionally exposed in the UI; it is not a second persistence schema.

Malformed user values preserve the existing safe fallback behavior and produce a testable diagnostic. Old files missing fields inherit embedded defaults, and obsolete unknown fields are ignored rather than making startup fail.

Alternative considered: serde field-level Rust defaults. Rejected because project policy requires defaults to live only in `client/assets/default_config.toml`.

### 7. Isolate DSH model-selection compatibility behind one adapter

Create `bridge/src/model-selection.js` as the sole bridge-owned API. During implementation, first inspect the supported DSH version for a stable exported `installModelSelection` and use it if available without increasing the injected host-service surface. If unavailable, retain a bounded compatibility implementation only in this adapter, exact-pin the tested DSH compatibility metadata, and add the deployed-copy smoke test to automated verification.

`session.js` depends only on the adapter. Contract tests assert assembly variables, request routing, reasoning-effort behavior, and disposer semantics. The broad declaration `>=rc.6 <0.2.0` is not allowed while using a copied private implementation.

Alternative considered: leave the copy in `compose.js` with comments. Rejected because comments and manual smoke instructions have not prevented metadata drift elsewhere and cannot constrain dependency resolution.

### 8. Preserve behavior with characterization gates before deletion

Before changing ownership, retain/expand tests for user and assistant cards, Thinking/reasoning, file folding, activity outcomes, copy atomicity, cache tail splice/range patch, history replace/prepend, Input Page focus, command completion, lock discipline, launcher stale locks, and wire frames. Each migration phase must pass the full Rust and bridge suites; only then may the old path be deleted.

The architecture audit is rerun after completion. Acceptance requires no production SCC, no legacy transcript variants/adapters, synchronized protocol facts, and deterministic runtime/launcher tests.

## Risks / Trade-offs

- **[Risk] Removing `Msg` can regress subtle copy/cache/history behavior.** → Add characterization tests first, migrate one display family at a time, and retain stable IDs/unit ownership until equivalence is proven.
- **[Risk] Module extraction creates a large noisy diff.** → Separate moves from behavior changes, keep façade re-exports temporarily, and require tests after every dependency-edge cut.
- **[Risk] A generic runtime controller can become another god object.** → Split bridge-message and terminal-event handlers by input family; the controller only orders handlers and effects.
- **[Risk] Source-scanning architecture tests may misparse unusual Rust syntax.** → Keep imports conventional, test the scanner itself, and use it only for module-level policy rather than semantic compilation.
- **[Risk] Contract shape metadata can drift from hand-written handlers.** → Generate conformance fixtures and fail both Rust and Node tests when generated outputs or message coverage differ.
- **[Risk] TOML value merging can alter malformed/unknown-field behavior.** → Lock current missing-field, unknown-field, invalid-type, and invalid-theme cases in tests before replacing loaders.
- **[Risk] Exact-pinning DSH reduces upgrade flexibility.** → Prefer the official exported helper; if pinning is necessary, make upgrade verification automated and explicit rather than accepting silent runtime drift.
- **[Risk] Full remediation is too large for one atomic implementation.** → Use the migration phases below; every phase is independently testable and leaves the application runnable.

## Migration Plan

1. Add characterization tests, module-edge/SCC detection, protocol drift assertions, and baseline timing checks without changing runtime behavior.
2. Extract `page_core`, `command_catalog`, and `transcript_layout`; cut the two existing SCCs and enable the acyclic architecture gate.
3. Introduce runtime effects/controller and launcher ports; move behavior out of `main.rs::run` and direct infrastructure calls while preserving production adapters.
4. Split HostEvent parsing, the surface coordinator, and UI page/transcript rendering behind existing façades; do not relocate legacy `Msg` family reducers merely to move them again.
5. Introduce the single display store and migrate each transcript family directly from `model.rs::reduce_host_event` into its final projection family module, then remove `Msg`, compatibility reducer paths, and event-specific UI adapters.
6. Extend/synchronize the wire contract, correct package version metadata, add conformance generation, and regenerate protocol docs.
7. Replace parallel config schemas with value-overlay loading and one strict `Config` deserialization.
8. Move model-selection installation behind its adapter; use the official export or pin and automate compatibility smoke verification.
9. Run `cargo test`, bridge tests, protocol generation checks, examples/benchmarks, live deployed bridge smoke, dependency SCC audit, and update all project documentation and completed OpenSpec records.

Rollback is phase-local: façade re-exports and characterization tests permit reverting the current phase without reverting prior acyclic extractions. No wire roster/version bump is required unless the additive shape metadata changes runtime negotiation; if a bump becomes necessary, older peers continue through the existing compatibility rules.

## Resolved Implementation Record

- `@deepseek-ai/dsh-agent@0.1.0-rc.6` 稳定地从 package root 导出
  `installModelSelection(agentCtx, selection) -> disposer`。`bridge/src/model-selection.js` lazy-load
  并复用该公开 export；`bridge/package.json` 精确钉住 agent peer，
  `tools/verify-dsh-upgrade.mjs` 同时检查 DSH host/agent 版本、export、contract 和 deployed routing。
- 轻量 module-edge scanner 实现为 Rust integration test `client/tests/architecture.rs`：它扫描 production
  `use crate::...` edges、运行 Tarjan SCC，并断言禁止反向边与 transcript/config legacy guard；因此
  `cargo test` 是权威架构门禁。
- `protocol-contract.json` 的 `shapeTypes`、`records` 与 `messageShapes` 已覆盖全部 client messages 与
  非-HostEvent server frames；`tools/sync-protocol-contract.mjs` 生成/校验 Rust constants、fixtures、
  package metadata 和 docs。HostEvent payload 继续由有界 typed parser 演进。
