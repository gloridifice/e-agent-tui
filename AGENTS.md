# AGENTS.md

Project notes for coding agents. Human readers should see [README.md](README.md); for design decisions see
[docs/design.md](docs/design.md) (D1–D30, protocol, milestones).

> **Language policy:** All project documentation — this file and everything under `docs/` — is written and
> maintained in **English**. When adding or updating documentation, write English prose; do not introduce new
> Chinese (or other non-English) prose. Code identifiers, file paths, and command names stay as-is.

## What this project is

Terminal client for DeepSeek Harness (DSH) (project name **e**, executable **`dshe`**), in two parts:

- `bridge/` — Node.js (ESM) DSH **host-composition plugin**. Registers one WS upgrade route
  (`/dsh-tui`), forwards session events to the TUI, and accepts input/commands/interrupt/approval answers/
  session switching/history paging, plus `/login` `/model` `/skill:<name>` bridging. The only injected
  dependency is `webServer`.
- `client/` — Rust (ratatui + crossterm) single-exe client (crate `e`, artifact `dshe.exe`). Includes the
  launcher (`launcher.rs`: probe/spawn `dsh --profile dshe`/bridge) and the theme system (`theme.rs` +
  `config.rs`). No TLS/network dependencies (except WebSocket itself).

The two processes communicate over JSON WebSocket; the only machine-readable contract is
`bridge/protocol-contract.json` (the sole hand-written source of truth for version/capacities/roster/
shapeTypes/records/messageShapes). `tools/sync-protocol-contract.mjs` syncs `docs/protocol.md`,
`client/build.rs` constants/shape JSON, Rust/Node conformance fixtures, and
`bridge/package.json.dshCompatibility.wireProtocol` from it; `--check` must pass after changing the contract.
`bridge/src/protocol.js` reads the same JSON at runtime; `tools/generate-protocol-doc.mjs` is only a
compatibility wrapper. Token auth; the token lives at `%DSH_HOME%\dsh-tui.token`.
Client config lives at `%APPDATA%\dshe\config.toml`; the default config source is
`client/assets/default_config.toml` (embedded via `include_str!` and parsed; the user TOML only overrides
known keys and is then deserialized through a single strict `Config` schema; missing fields inherit,
deprecated unknown keys are ignored, malformed/known-type errors fall back safely); themes live in
`%APPDATA%\dshe\themes\`.

## Common commands (Windows / PowerShell)

The user-facing source install flow is documented in the README "Quick Start": install
`@deepseek-ai/dsh` globally, set/reuse `DSH_HOME`, mount and install the dedicated `dshe` profile bridge,
then use `cargo install --path client --locked` to install `dshe.exe` into the Cargo bin directory.

```powershell
# First install
npm install --global @deepseek-ai/dsh
# When DSH_HOME is unset/empty, the mount script falls back to $HOME\.dsh; set the env var first only for a custom home
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install
cargo install --path client --locked

# Client (repo root is a Cargo workspace, default member client, crate name e, artifact dshe.exe)
cargo run                                # build from root and launch dshe
cargo build --release                    # artifact target\release\dshe.exe
cargo build --release --features tracy   # Tracy profiling build (activated by DSH_TUI_TRACY=1)
cargo fmt --check
cargo clippy --all-targets
cargo test                               # full unit tests

# Bridge sync (required after changing bridge/; takes effect after restarting dsh)
.\tools\mount-bridge.ps1 -Profile web    # or -Profile dshe (the dshe launcher's dedicated profile)
# Equivalent manual command: robocopy bridge\src "$env:DSH_HOME\profiles\<p>\packages\dsh-tui-bridge\src" /MIR
# The script must be compatible with Windows PowerShell 5.1: empty DSH_HOME falls back to $HOME\.dsh;
# variable names are case-insensitive; Node JSON must be UTF-8 without BOM

# Bridge tests (node:test; includes protocol/session/model-selection edges)
cd bridge; npm test                      # = node --test --test-isolation=none "test/*.test.js"
node tools/sync-protocol-contract.mjs --check
# After a DSH upgrade or bridge change: mount + dsh plugin install, then run the full compatibility gate against the deployed copy
$env:DSH_TUI_SMOKE_PROFILE = 'dshe'; cd bridge; npm run verify-dsh-upgrade

# Integration debugging
node tools/probe-online.mjs       # is the bridge online
node tools/hello-test.mjs         # send hello and print all frames (verify the startup path)
node tools/probe-startup.mjs      # attach latency / snapshot size
node tools/dump-snapshot.mjs      # capture a snapshot sample -> tools/cache/snapshot-sample.json
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json
cargo run --release --example timing_frames # 1002-message continuous scroll/stream/animation frame benchmark
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

cargo uses the official crates.io registry (local network is fixed). `client/vendor/` and
`tools/vendor-crates.mjs` are legacy offline fallbacks, now retired — do not depend on them again; add new
dependencies directly to `client/Cargo.toml` and commit the root `Cargo.lock` (workspace lockfile).

## Key architecture conventions

### client (Rust)

- **Event display model** (`display.rs` + `projection/{store,assistant,tool,lifecycle,retry,command,workflow,surface}.rs` +
  `transcript_layout.rs`): all visible events fall into four public surfaces: `ActivityRow` (with
  Waiting/Running/Success/Failure/Cancelled state, optionally with parent/depth), `TranscriptBlock`
  (plain/markdown/reasoning/unknown fallback), `ContentCard` (uniform padding/background/copy source), and
  `InputAccessory` (above the input bar). Production `AppState` holds **only** `TranscriptStore`;
  `EventProjector` first produces display/surface mutation/page state/accessory/ignore effects, which the
  state layer then applies; adding event-specific top-level rendering in `ui` that bypasses the public
  surfaces is forbidden. `LegacyTestMsg`/`Msg` alias may only appear in `#[cfg(test)]` characterization
  fixtures and must not re-enter production transcript, renderer, cache, or copy paths.
- **Reasoning output folding**: `TranscriptFormat::Reasoning` blocks are not rendered to screen in compact
  mode and do not enter copy provenance (production `ui/transcript.rs::is_hidden_item` makes layout/cache/copy
  skip them, without producing an inter-row gap); activity-row adjacency must look up the next **non-hidden**
  DisplayItem — hidden reasoning must not split apart activity rows that should be glued together.
  lines/full mode renders reasoning content directly: the lines cap is the **post-wrap display row count**
  (width-aware wrap happens before `thinking_lines` truncation); and whenever reasoning is visible, the
  immediately preceding `Thinking...` activity row is taken over and hidden by `thinking_row_superseded`
  (not rendered, no gap). The hidden determination must be uniform across rendering/copy/adjacency/hiding
  itself (`is_hidden_node`); animation patches skip hidden nodes and must **not** fall into the full-rebuild
  fallback. Thinking is represented by a `• Thinking... xN` breathing indicator; `assistant/chunk` carrying
  only reasoning does not settle until the real answer text arrives, which settles to green. Therefore
  Thinking settlement must search backwards in `TranscriptStore` for a Running Thinking activity — never
  assume the last node is visible.
- **Context injection card**: the visible content of `CardRole::Context` shows at most 5 lines under
  width-aware wrap; if it overflows, the 5th line is replaced with `...`. The card's `copy_source`/copy unit
  must preserve the complete original text and must not be truncated by the display clip.
- **File activity folding**: `FileGroup` keeps the `read/view/edit/replace/insert` labels via a unified
  `FileItem + FileAction`; consecutive `str_replace_editor` view/str_replace/insert and read/edit calls enter
  the same folded activity row; the editor's absolute path is converted to a workspace-relative path using
  `session_cwd`. create does not enter FileGroup and is shown separately as
  `<indicator> create <relative-path>`, and does not append output line count/elapsed time after completion.
  All activity rows stay on a single display row; when too wide, `transcript_layout`/`ui::transcript`
  truncate and append `…` using the resolved page content width (including `page_max_width`) — never
  pre-truncate to terminal width and then wrap inside a narrower page.
- **Surface semantics**: `HostEvent` parses the event top-level `time`, `surfaceOp`, `sourceEventSeqs`;
  replace must first remove the shadowed surface owner, then insert the replacement node at the original
  surface position. Unknown events that carry `surfaceOp` must also enter the snapshot/history compatibility
  path. On history prepend, save the shadowed seq so later older pages cannot revive compacted content; when a
  tool/command/Code Mode/workflow terminal half is split from its start by a page boundary, stage it and
  rebuild the final state directly when the older page's start arrives; when a retry schedule and a newer
  retry-started span a page boundary, backfill delay/failure/maxRetries after restoring saved rows — do not
  drop details just for dedup; move the viewport only by the truly newly added rendered rows. workflow
  completed/failed/cancelled must be kept as a typed outcome and mapped to Success/Failure/Cancelled.
  compaction's log-only summary is not drawn as its own card; the single summary card is created and owned by
  the replacement, so a later replace can delete it precisely.
- **Render cache** (`cache.rs::TranscriptRenderCache` + `transcript_layout.rs` + `ui/transcript.rs`): only
  structural events invalidate the cache and trigger a full rebuild; streaming chunks only set `tail_dirty`,
  and rendering **splices the tail** and recomputes only the tail display-row suffix/prefix — never clear the
  entire layout; spinner/settle only patch the active `DisplayId` range, and settle must submit one more
  precise target-color patch after expiry before stopping the clock. Copy line numbers come from the same
  `TranscriptLayout` the UI uses; the main loop uses `CopyRowsCache` keyed by width/generation to reuse
  provenance — never fully `flatten` on each copy keypress and each subsequent frame. Wrap scanning computes
  display width by Unicode grapheme cluster; combining marks / emoji ZWJ must not be split even across style
  spans.
- **Performance red lines** (all have regression tests): terminal input wakes the main loop directly through
  `EventStream` — do not restore fixed ticker polling; interaction/content/animation deadlines are separated,
  and the bridge backlog is bounded per turn by a count+time budget. The terminal is initialized/restored at a
  single point via `terminal_runtime.rs::TerminalOwner`; frames are committed atomically with a 64KiB
  `BufWriter` + DEC 2026 synchronized output (`DSHE_DISABLE_SYNC_OUTPUT=1` only as a compatibility diagnostic).
  Never full-render per event; redraw P95 ≤30ms and only when dirty/deadline expires; animation only patches
  the active message range, streaming only splices the tail; display-row layout is cached by width/generation,
  and each frame only materializes/clones the visible window. Do not break the shared layout semantics of
  `valid/tail_dirty/dirty_messages`, history display-row anchor, and copy provenance.
- **Runtime controller / lock discipline**: `runtime.rs::RuntimeController` receives typed `RuntimeInput`,
  consumes `ControllerAction` inside a single scoped guard, and hands only complete-payload `RuntimeEffect`s
  to the `main.rs` executor; `runtime_ports.rs` provides transport, terminal, config/state, clipboard, and
  clock production/scripted ports. The executor must not borrow UI state or silently ignore effects; do not
  restore a fixed ticker. In Rust 2021, `if let`/`match` scrutinee temporaries live until the end of the whole
  expression; never write `state_r.lock()` directly into a scrutinee and then re-lock or `.await` in a branch,
  or you will self-deadlock. Compute plain values/actions in a separate scope before matching, or perform
  atomic state changes within a single guard; `main.rs` already denies `clippy::significant_drop_in_scrutinee`
  and has queue-dispatch/copy-mode lock-release regression tests.
- **Input interaction and character boundaries**: `InputState.cursor` is a **character index**;
  `String::insert/remove` and slicing need byte indices — use `char_to_byte()` (`input.rs`); CJK has regression
  tests; cursor x uses `unicode_width`. Plain input is fixed: `Enter` sends, `Shift+Enter` inserts a newline;
  `↑/↓` move between input lines by character column first, and only switch to the previous/next history prompt
  at the first/last line boundary; `PageUp`/`PageDown` page by the currently visible transcript height, and the
  mouse wheel moves 3 lines per notch (always operating on the transcript even when an Input Page is open).
  `Ctrl+H` is a global help key handled before the Input Page, and `hjkl` with Control/Alt/Super must not enter
  the focus graph. `Config.enter_sends` exists only for legacy config deserialization compatibility and must
  no longer change key semantics. The terminal hardware cursor must always be hidden inside the TUI; the screen
  only draws a software reverse-video cursor; `ui.rs::render_with_cursor` only returns the IME anchor, and the
  main loop moves the hidden cursor after the frame completes. Do not call `Frame::set_cursor_position` again —
  it makes ratatui show and drag the cursor during diff drawing, causing the status light/input bar to flicker.
- **Overlays and Input Page rendering**: a command prompt that truly draws over the transcript must first call
  `frame.render_widget(Clear, rect)` before drawing the background, otherwise underlying text bleeds through
  (there is a test `suggest_popup_is_opaque_over_transcript`). `/settings` `/login` `/model` `/theme` `/resume`
  are not overlays: they are uniformly handled by `InputPageSession` replacing the input area, no border, no
  `Clear`, with the shared shell fixed at 1 row top/bottom and 2 columns left/right padding.
- **copy semantics**: copy always takes the original markdown (`units` table); tables/code/mermaid are atomic
  blocks (`RenderLine.atomic`). Render unit ids are reused across re-renders (`unit_start`) — do not reassign
  them.
- **Markdown headings and localized backgrounds**: headings directly use the fixed semantics
  `semantics.markdown.heading1..6`; in ferra, level 1 is Coral `#ffa07a` bold (no background), level 2 is Sage
  `#b1b695` bold, level 3 is Blush `#fecdb2` non-bold. inline code `bg` may only apply to the chip span;
  `render_transcript` only lets `Line.style.bg` trigger full-line fill — never infer a full-line background from
  an arbitrary span's background, or you will pollute source separator spaces and trailing whitespace. Changing
  these styles must sync the built-in theme TOML, `render.rs`, and TestBackend regression tests.
- **Table cells**: must go through `cell_spans()` (`render.rs`) for inline rendering + display column-width
  truncation/padding — never stuff bare strings in.
- **History paging**: `min_seq`/`history_loading`/`history_exhausted`; prepend goes through `prepend_events`
  (sets `prepend_line_anchor`, and the renderer shifts `scroll.offset` by the truly newly added display rows to
  keep the viewport). The top "history" hint row is **display-only** and does not enter the cache; Thinking is a
  public `ActivityRow`, but is not generated during snapshot replay/history prepend (`state.replaying`), and
  file-group merge/settlement scans skip it.
- **Tracy/timing** (`profile.rs`): instrument with `e::tracy_zone!("literal")` (a macro that safely no-ops when
  no client is present); use `PhaseTimers` for stage timing. Zone names must be string literals.
- **Bottom layout and two-line status bar**: the fixed bottom row order is input bar or Input Page / gap /
  status line 1 / **session title line** (the `ui.rs::render` chunks array; the `+3` in the accessory budget
  formula matches it). Neither line sets a background color: line 1 is, left to right, the working indicator,
  `AppState.current_mode`, the current model, and `CH<cache-hit %>`, where the model and CH entries are omitted
  entirely when they have no value yet (no placeholder dash), and the right side is fixed `^h Help`; line 2's
  left side is `AppState.session_title` (shows `新会话` when empty) and the right side is the absolute
  `AppState.session_cwd` path, with the title truncated with `…` when too long so the path is preserved.
  mode's initial value comes from `welcome.mode` (most recent selection, else the creation header), then is
  updated by `agent-preset/selected` replay, keeping the latest value by event seq (history prepend must not
  regress it); CH accumulates from assistant usage input/cache read/cache write, where history prepend may add
  older totals but must not replace the latest request's usage anchor; these page-state updates must **not**
  touch `TranscriptRenderCache`. When changing the bottom row count, sync the hardcoded line numbers in the UI
  layer tests.
- **Command paradigm** (`runtime_command.rs` + `input.rs`): commands are split into built-in optimized commands
  and DSH integrated commands. All built-ins are declared exactly once in `BUILTIN_COMMANDS` (name/description/
  input hint/completion strategy/action in one entry) — never maintain a parallel name table in `input.rs`;
  `match_command_catalog` merges the `CommandInfo` sent by the bridge, with built-ins winning on name
  collision. Integrated commands come from each agent's effective `ctx.commands.list` view, and at minimum
  support fuzzy name completion and show DSH's free-form input hint; DSH currently has no typed argument
  completion schema, so only built-ins can do argument completion like `/new `; `/skill` is another built-in
  argument completion — typing the full `/skill` shows the current user-invocable roster and fills candidates as
  `/skill:<name>`. On receiving a new `commands`/`skills` frame, refresh any open prompt immediately; on session
  switch, clear the old agent-scoped catalog first. Generic execution must not pre-`start_thinking`; the result
  is projected directly to System/Error by `command-result`.
- **Startup and deferred `/new`**: a new process sends hello without `resumeSessionId`, and the bridge still
  creates a session in place (`hello.cwd` workspace + `hello.mode` default mode, falling back to standard on
  failure); only a CLI session id and "remember last session" (default off) resume. `/resume` opens the resume
  Input Page, `/resume <id>` attaches directly. A bare `/new` while interactive only creates a client-side
  `NewConversationDraft` (display name `新对话`) — it does not send to the bridge or replace the real session
  id/TranscriptStore; only the first plain input sends the atomic `new-input{mode,text}` to create and deliver.
  During the draft, old-session frames keep reducing but are not displayed; on create failure restore the input;
  `/model`, `/skill`, and integrated commands must not be misrouted to the old session.
- **Input Page controller** (`input_page.rs` + `settings.rs` + `login.rs`): the main loop holds a single
  `Option<InputPageSession>` with the closed variant set Settings/Login/Model/Theme/Resume; page keys only return
  `PageOutcome`/`PageEffect`, and the caller saves or `.await`s sending only after releasing the page borrow and
  state lock. Browse-state arrow keys and `hjkl` share a stable focus graph, Enter executes; text-edit-state
  `hjkl` must be ordinary characters. Dynamic login/model/session rosters reconcile focus by
  provider/model/proxy/session id, and an empty list must not fabricate a fake focus; Resume always uses plain
  characters (including hjkl) for title/id filtering, with only ↑↓ selecting a session.
- **/login page**: a one-level two-choice menu (API key / Proxy) → sub-pages (Menu / Providers / ApiKey /
  ProxyList / ProxyForm / ProxyDelete). State comes from bridge `login` frames; the API key is never sent back
  and is drawn as ● when editing; non-writable providers must not receive action focus; an existing proxy must
  enter the delete confirmation page on Enter, and `login-proxy-delete` is only sent after explicitly choosing
  delete.
- After adding interaction keys, sync: `ui.rs`'s `help_overlay`, the README quick-reference table, and input
  tests.

### bridge (Node.js)

- **Module layout**: `index.js` only keeps WebSocket lifecycle and composition wiring; `dispatcher.js` is the
  client frame router; `host.js` explicitly wraps the DSH service locator, `connection.js` unifies detach/
  expired-connection determination, `history.js` manages surface cache and paging, `session.js` manages
  create/cold-resume/workspace/preset composition, `session-list.js` manages the progressive session catalog and
  title folding, `model-selection.js` is the sole DSH model-selection adapter, `command.js` projects the host
  command catalog/direct results, `protocol.js` reads the shared wire contract; `trim.js`, `compose.js`,
  `login.js`, `skill.js`, `model.js`, `frame.js` keep their own pure logic. Every boundary must have
  `node:test` under `bridge/test/`; new code must not pile back into `index.js`.
- **DSH command integration**: after attach, use `ctx.commands.list(agent)` to send handler-free
  `commands{commands[{name,description,input?:{hint}}]}`; on `commands/change`, recompute the effective catalog
  for each connection (agent-scoped shadowing cannot be done as a global incremental patch). `command{line}` goes
  through `commands.execute(agent,line,signal)`; `undefined` means unregistered/invalid syntax, and the settled
  result goes through `command-result{commandId,kind,text?}` — never becomes a model message. Execution spans
  awaits, so a current-conn check is required before returning the result to avoid cross-session leakage after
  attach.
- **Cross-await conn discipline**: in any message handler that touches `conn` (detach/rebind) after an `await`,
  capture a local `current = conn` before the await and verify `conn === current && conns.has(current)` after it
  before operating — consecutive attach/`/new` swaps the closure's `conn` concurrently, and operating on a stale
  connection leaks across sessions (the `attach` branch has a reference implementation).
- All side effects go through `ctx.effect()`; connection objects live in the `conns` set; detach must clean up
  listeners and revoke pending approvals (`done('cancelled')`).
- **Snapshot/history data sources**: active sessions read `agent.session.events` (in memory, zero disk reads);
  only non-resident sessions fall back to `persistence.readFrom(id, 0)` (full disk read, slow, result cached into
  `conn.log`). After that async read you must first check `conn.abort.signal.aborted`; a stale connection must
  not be written to or sent a snapshot. The surface list is cached per session and appended incrementally
  (`surfaceState`), and besides the contract roster it also keeps any unknown event that explicitly carries
  `surfaceOp`; unknown events must be trimmed to a bounded type/seq/time/surface metadata envelope before
  entering the wire — never carry arbitrary data. All downstream frames go through
  `frame.js::encodeBoundedFrame` enforcing `MAX_FRAME_BYTES`: snapshot/history keep only the newest fitting
  suffix and set truncated/hasMore, and a single over-limit frame becomes a `frame-too-large` error.
- **Payload trimming**: `trimToolResultEvent` (a module-level pure function exported as `_trimToolResultEvent`
  for tests) — read results are stripped entirely, other tools keep only the last 2000 characters (the exit
  marker is at the end). Real-time event forwarding must also go through it — do not restore full forwarding.
- When changing the message roster, surface types, capacities, or payload shape, only change
  `bridge/protocol-contract.json`, then run `node tools/sync-protocol-contract.mjs`; it validates and generates
  docs, Rust constants/shape JSON, Rust/Node fixtures, and package wire metadata, and `--check` must pass.
  Still also update the payload structures in `client/src/protocol/` (serde camelCase) and the bridge handler,
  and add contract-driven tests on both sides. Session events must first be parsed into `HostEventKind` — never
  let `serde_json::Value` into the reducer. The history roster only includes the events needed to rebuild the
  supported display/input accessories; approval/request/header/title-llm and similar audit or rebuild records
  are not in the transcript by default. Tool results, Code Mode sub-calls, compaction summaries, and `meta` must
  all be bounded-trimmed, with `data.dshTuiTrimmed: true` after trimming; the client must not pass off trailing
  line counts as complete output line counts.
- **Blank-session history and `/new` workspace inheritance**: `/resume` judges blankness by the presence of
  `turn/start`, and session eligibility comes before the 200-entry cap; active sessions read memory, cold
  sessions prefer the `sessionListMetadata.blank` projection/cache and then degrade to `readFrom`, failing open
  on error. Old setup-only logs are not deleted but do not enter history. Materializing a new session must do
  two things together — `agents.create`'s `meta.cwd` points at the target directory, then find that cwd's
  workspace via `ctx.get('workspaceRegistry')`'s `resolveByPath` (or `create` if absent) and
  `attachSession(agent.id)`. With only the cwd header and no attach, the session will not enter the workspace's
  `sessionIds` ledger (host's own `session.create` also does both steps). **cwd priority**: client `hello.cwd`
  (the TUI's launch directory, validated on the bridge side with `isExistingDirectory`) > current session
  `header.cwd` > `process.cwd()` — `/new` lands in the workspace of whichever directory the TUI launched in.
- **`/new <mode>` and mode prompts**: `/new` is a client draft command; the first input materializes atomically
  as wire v5 `new-input`; the bridge still keeps the `/new` command handler for older clients (it is not in the
  DSH command registry). dshe's bare `/new` builds a draft using client `Config.default_mode`; when the bridge
  receives a genuinely bare `/new` from an older client, it compatibly inherits the current session preset
  (`agentPresets.composedPreset(current.ctx)`, falling back to `header.agentPreset`, then the roster default).
  `/new <mode>` resolves directly by preset id (`agentPresets.resolve`, unknown names error with an available
  list). New sessions must `agentPresets.mount(agentCtx, preset.id)` inside `agents.create`'s `setup` — writing
  only the `meta.agentPreset` header without mounting leaves the session without the preset's tools/prompts
  (consistent with host `session.create` composition). After attach the bridge sends a `presets{presets[]}`
  roster frame (id/name/description/order/broken), which the client uses to render the mode prompt popup after
  `/new ` (broken presets are not sent).
- When a connection object is reused across sessions (`/new`, Resume attach), it must keep `conn.clientCwd`,
  otherwise workspace inheritance degrades back to header.cwd after reconnect.
- **hello creates a session immediately**: when hello carries no `resumeSessionId`, the bridge calls the same
  `createNewSession(ws, null, mode, { clientCwd, fallbackStandard: true })` to create a session in place (`conn`
  is null, no mirror/detach); `fallbackStandard` degrades the `resolve` failure chain to `standard` → roster
  default, for /settings "default mode" invalidation fallback. When carrying a `resumeSessionId` (or Resume/
  `/resume` attach) whose id is not in the active registry, first `resumePersistedSession`:
  `sessionPersistence.list/inspect` + `agents.resume`, taking the preset from the session record
  (`sessionPresetOf`: most recent `agent-preset/selected` > `header.agentPreset`) and mounting it via
  `agentPresets.mount` in the resume `setup` (history must not replay under a different composition; if the
  preset is gone, don't restore) — if none of these work, create a new session, and **never close the connection
  for any "session not active"** (restart races are not user errors). Only a create failure sends a
  `hello-failed` error frame + `ws.close(4001)`.
- **Session title**: `welcome.title` takes the most recent `session/title` in `agent.session.events`
  (`latestTitle` pure function); cold-resumed session logs are not in memory, so after attach the bridge
  re-sends a `title{title}` frame via `sessionQuery.readTitleSnapshots` (verifying before sending that the
  connection is still attached to the same session, to prevent cross-session title leakage); later title updates
  need no dedicated push — `session/event` full forwarding already brings `session/title` events to the client.
  `session/title` does **not** enter `SNAPSHOT_SURFACE`: history prepend replays via `apply_event`, and an old
  title would overwrite the new one. The workspace path goes through `welcome.cwd` (the session header
  `header.cwd`, sent with welcome at attach), and the client stores it in `session_cwd` to render on the right of
  the title line. `sessionQuery.readTitleSnapshots` returns a settled result; the title must be unwrapped from
  `fulfilled.value.title.title`; `list-sessions` first sends at most 200 header/online-title
  `sessions{titlesPending:true}`, then asynchronously sends persisted titles, and reads titles only for the
  truncated candidates.
- **model selection must be installed**: every session the bridge creates/resumes must first call the
  `model-selection.js` adapter's `install(agentCtx, { current, assembled })` inside `setup`. The adapter
  lazy-loads and reuses the `@deepseek-ai/dsh-agent@0.1.0-rc.6` public package-root `installModelSelection`
  export — the production bridge must **not** copy a waterfall; it is responsible for the
  `system-prompt/assemble` `variables.{provider,model}` injection and post-snapshot `agent/request` routing,
  otherwise the persona's `{{model}}` has no value. `current` takes the `/new` mirror's current session
  provider/model (`mirror`), else `agentDefaultModel.currentSelection()`; adapter install and preset mount are
  two orthogonal steps, and the adapter runs first. After changing DSH/compatibility metadata you must
  mount/restart, then run `npm run verify-dsh-upgrade`; it checks the canonical contract, exact host/agent
  version/export, deployed helper, and `/new`/cold-resume/`/model` routing.
- **/login field destinations** (bridge `login.js`): upstream `login-get` / `login-set-api-key` /
  `login-proxy-create` / `login-proxy-delete`; downstream `login{providers[],proxies[],error?}`.
  - API key: `ctx.llm.listProviders()` lists providers; `providerCredentialRef` reads `apiKeyEnv` from settings
    (defaulting to `<ID>_API_KEY`), going through `ctx.credentials`' `describe/set/unset(ref)` (**the value is
    never sent back**, only a `…last four` hint is sent; env sources are read-only).
  - Proxy: stored in `%DSH_HOME%\dsh-tui-proxies.json` (api key not sent back).
  Write failures return to the panel via the same `login` frame's `error`, not through the transcript error
  stream.
- **/model (bridge)**: upstream `model-get` / `model-set{provider,model}`; downstream
  `model{providers[{id,name,models[{id,name,description?}]}],current?}`. `sendModel` uses
  `ctx.llm.listProviders()` + `ctx.llm.listModels(id)` (a provider without an adapter catalog returns an empty
  list rather than failing the whole thing). On session create/resume, store that `{current,assembled}` pair via
  `modelSelections.set(agent.id, selection)`; `model-set` changes `selection.current` (effective on the next
  `system-prompt/assemble`) and also updates `agent.options`, then replies with a `model` frame to refresh the
  client status bar.
- **/skill (bridge)**: `/skill:<name>` or `/skill <name>` is intercepted by the bridge (`skill.js`'s
  `parseSkillCommand`). After each attach and `skills/change`, call `ctx.skills.list` by session cwd/scope and
  send only `invocation.userInvocable` `{name,description}` via the `skills` roster; the client starts fuzzy
  completion when the full `/skill` is typed and fills the canonical colon form. Execution looks up the skill via
  `ctx.get('skills').get(name, {cwd, signal, scope})` — **DSH's skill-filesystem already discovers with
  `<workspace>/.agents/skills/` > `~/.agents/skills/` priority**; the bridge only injects
  `renderSkillContent(skill)` (the `<skill_content>` block) into the session via `createUserMessage` +
  `source:{kind:"skill-invocation"}` `followup` (mirroring dsh-tool-skill's explicit user invocation injection);
  unknown names return `error{code:"skill-unknown"}`. Verify `conns.has(current)` after the await.
- **Config/theme/launcher (client)**: config defaults live only in `client/assets/default_config.toml`,
  embedded and parsed by `config.rs` via `include_str!`; the persisted `Config` is deserialized directly with
  `Deserialize` + `#[serde(deny_unknown_fields)]`, and `resolved_theme` is a `#[serde(skip)]` runtime cache.
  `from_user_toml` first recursively `overlay_known`s user values onto the embedded TOML as the schema, then
  strictly deserializes exactly once: old files inherit missing fields, deprecated unknown keys are ignored,
  malformed/known-type errors fall back safely; `Config::default()` must not re-derive from Rust field literals.
  `Config.theme` stores the theme name; rendering does zero disk reads. Themes are two-layer TOML: an open
  `[colors]` allows arbitrary color names, and fixed `[semantics.*]` (surface/markdown/input/working_status/log/
  activity/card/overlay) link semantic styles to color names; each style requires only `fg`, with `bg`/`bold`/
  `italic`/`underline` optional; unknown references, missing fixed fields, or illegal hex reject the whole file.
  Built-in `deepseek-e`/`ferra` sources are in `client/assets/themes/`, embedded via `include_str!` and parsed by
  the same parser as user files, and copied without overwrite to `%APPDATA%\dshe\themes\`; a valid same-named user
  file wins, and an illegal old file must not shadow the embedded fallback. `launcher.rs`: `probe(url)` TCP probe
  → if no dsh, spawn `dsh --profile dshe` (`dsh` or `npx @deepseek-ai/dsh`) → `%DSH_HOME%\dsh-tui.lock` counts
  "close dsh when the last tui closes"; on Windows the child handle points at the `cmd /C` shim, and both normal
  shutdown and startup-timeout cleanup must `taskkill /T` the whole process tree — never only `Child::kill`,
  which leaves orphan Node processes; child reaping must be bounded, and on terminate failure keep an
  `instances: 0` lock for the next attach to retry; reading any lock must re-`probe(url)` — even `instances > 0`
  is not proof of a live service (a force-killed TUI leaves a stale positive-count lock), and if the service is
  gone, clear the lock and rebuild. `release` returns `true` only when it actually shut down a managed service,
  and after the main program exits the alternate screen it prints `dsh 服务器已关闭。`. The launcher must use the
  dedicated `dshe` profile and must not reuse DSH's own / user's existing `tui` profile (whose terminal UI grabs
  stdio and does not provide the `webServer` the bridge depends on). Hello-terminal bridge errors (`protocol-newer`,
  `bad-token`, `hello-failed`) must become actionable fatal client errors before the following WebSocket close can
  overwrite them with a generic disconnect; protocol mismatch guidance must mention remount/install/restart.
  `/reload` re-reads config + rescans themes.

## Maintenance discipline

- **Maintain all documentation in English.** AGENTS.md and everything under `docs/` are written in English and
  must be kept in English: write new or edited prose in English, and do not introduce Chinese (or other
  non-English) prose. Code identifiers, file paths, and command names stay as-is.
- For small and medium Rust tasks, do not run `cargo fmt --all` or `cargo clippy` at the end; for large tasks,
  run `cargo fmt --all` and `cargo clippy` at the end. Regardless of task size, `cargo fmt --all` must pass
  before committing.
- After a task, sync this file (AGENTS.md) and related `docs/` (e.g. design.md) to the scope of the change;
  descriptions of features, interaction keys, protocol fields, config defaults, or command lists must not lag.
  Key changes must also sync `ui.rs`'s `help_overlay`.
- **Do not update `README.md` unless necessary, and keep it concise.** Only update README when user-facing
  basics materially change — install/build flow, core user-visible capabilities, or keybinding quick reference;
  implementation details, architecture notes, protocol details, and development records belong in `docs/`, not
  in an expanded README.

## Test discipline

- Add tests cautiously: only when genuinely necessary, when they cover real risk or prevent regression; do not
  add tests for formal coverage's sake.
- Unless the user explicitly asks, avoid running the full `cargo test`: prefer running only the local tests
  relevant to the change; skip tests for small changes. Rendering/spacing changes must have UI-layer regression
  tests (TestBackend asserting cached line counts/colors/content), not only model-layer tests.
- Known flake: full parallel tests occasionally flake once (tool card assertion); a single or rerun passes — do
  not make big changes based on it.
- The bridge side has `node:test` (`bridge/test/`, `cd bridge && npm test`, using `--test-isolation=none` to
  avoid sandbox spawn EPERM): besides trim/compose/login/skill/model/model-selection,
  host/connection/history/session/session-list/protocol/dispatcher edges must also be covered; file-layer tests
  use a temp home (do not touch the real `%DSH_HOME%`). Protocol changes must also run
  `node tools/sync-protocol-contract.mjs --check`. After a DSH upgrade, mount + `dsh plugin --profile <p>
  install`, then run `npm run verify-dsh-upgrade` from `bridge/` (set `DSH_TUI_SMOKE_PROFILE=<p>` if needed);
  it smokes public exports, helpers, `/new`, cold resume, and `/model` routing against the deployed copy rather
  than relying on a local waterfall copy.

## Known issues

- **Restart DSH to load a new bridge**: after changing `bridge/src`, re-mount
  (`.\tools\mount-bridge.ps1 -Profile <web|dshe>`, equivalent to robocopy) + user restarts dsh. The old bridge's
  startup full disk read is ~14s; the new bridge's active-session path is <100ms.
- Design-doc M milestone numbering has fallen behind the implementation (features exceed M6); code and README
  are authoritative.
- Under `DSH_TUI_TIMING=1`, per-stage startup timings print to stderr, for locating startup regressions.
