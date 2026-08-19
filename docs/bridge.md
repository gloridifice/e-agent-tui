# bridge (Node.js)

Architecture conventions for the Node.js (ESM) DSH host-composition plugin.

- **Module layout**: `index.js` only keeps WebSocket lifecycle and composition wiring; `dispatcher.js` is the
  client frame router; `host.js` explicitly wraps the DSH service locator, `connection.js` unifies detach/
  expired-connection determination, `history.js` manages surface cache and paging, `session.js` manages
  create/cold-resume/workspace/preset composition, `session-list.js` manages the progressive session catalog and
  title folding, `model-selection.js` is the sole DSH model-selection adapter, `command.js` projects the host
  command catalog/direct results, `question.js` owns API-proxy question relay lifecycle, `protocol.js` reads the
  shared wire contract; `trim.js`, `compose.js`,
  `login.js`, `skill.js`, `model.js`, `frame.js` keep their own pure logic. Every boundary must have
  `node:test` under `bridge/test/`; new code must not pile back into `index.js`.
- **DSH command integration**: after attach, use `ctx.commands.list(agent)` to send handler-free
  `commands{commands[{name,description,input?:{hint}}]}`; on `commands/change`, recompute the effective catalog
  for each connection (agent-scoped shadowing cannot be done as a global incremental patch). `command{line}` goes
  through `commands.execute(agent,line,signal)`; `undefined` means unregistered/invalid syntax, and the settled
  result goes through `command-result{commandId,kind,text?}` — never becomes a model message. Each execution owns
  an `AbortController`; `interrupt` aborts all active command controllers as well as the agent turn, and the client
  keeps Esc interruptible while direct commands are unsettled even if agent status is idle. Execution spans awaits,
  so a current-conn check is required before returning the result to avoid cross-session leakage after attach.
- **User-question relay**: `apiProxy` is optional when the bridge initially composes, so `question.js` must open
  its RPC-enveloped mux under `ctx.inject(['apiProxy'], ...)`, not from an eager `ctx.get('apiProxy')`. The child
  injection owns stream cleanup across service reloads; answer dispatch resolves the current service lazily.
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
  Still also update the payload structures in `crates/e-dsh/src/protocol/` (serde camelCase) and the bridge handler,
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
