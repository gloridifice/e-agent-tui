## Context

`e-tui` already owns the normalized frontend state, projection, rendering, input pages, Preview, Reading View, themes, and configuration values. `e-dsh` still owns the concrete terminal loop and a transitional normalized-event reducer/controller in addition to its DSH adapter. Pi 0.84.3, the version currently installed in the development environment, documents `pi --mode rpc` as the preferred integration path for non-Node applications. RPC mode uses strict LF-delimited JSONL and already composes Pi's native settings, credentials, model runtime, resources, project-trust policy, extensions, and SessionManager.

The independent research document recommends a custom Node SDK host. That remains a possible later distribution architecture, but it would duplicate the existing RPC command/event surface, add an npm build/install lifecycle, and bind the first implementation to fast-changing SDK factory APIs. The source-install workflow can instead require the supported `pi` executable exactly as `dshe` requires DSH tooling.

## Goals / Non-Goals

**Goals:**

- Deliver a usable `pie` executable backed by Pi's native agent runtime and `e-tui` presentation.
- Preserve Pi-owned semantics and persistent state; Rust only adapts RPC records and renders UI state.
- Support startup/history snapshots, prompting and queueing, abort, new/switch session, model and thinking selection, compaction, resource command discovery, streaming text/reasoning, tools, retry/compaction lifecycle, and the RPC-compatible Extension UI subset.
- Keep Pi protocol values and child-process I/O out of `e-tui`.
- Fail with actionable guidance when `pi` is unavailable, incompatible, exits, or writes malformed protocol output.

**Non-Goals:**

- Reimplementing Pi's agent loop, resource loaders, package manager, settings merge, auth store, or session writer in Rust.
- Bundling Node or Pi in the first source-install release.
- Supporting Pi-TUI-only custom components, headers, footers, raw terminal hooks, or theme renderer APIs.
- Implementing OAuth/login inside `pie`; native `pi /login` remains the credential-management path.
- Adopting experimental `pi-protocol`/CBOR packages or a persistent daemon.

## Decisions

### Use the documented Pi RPC CLI as the sidecar

`pie` spawns `pi --mode rpc` in the launch cwd, optionally adding a session argument and an explicit one-run trust override. Child stdin/stdout carry protocol records; stderr is captured for diagnostics. The process is a child of `pie` and is terminated/reaped when the TUI exits.

Alternative: a repository-owned Node SDK host. Rejected for v1 because RPC already exposes the required cross-language contract and Extension UI semantics. A custom host becomes justified only when a required operation is absent from RPC and cannot be supplied safely as read-only metadata.

### Treat RPC stdout as strict protocol

The process layer splits only on byte `0x0A`, strips one trailing `0x0D`, enforces a bounded record size, parses each non-empty record as JSON, and reports malformed records as fatal protocol errors. Requests carry generated IDs where correlation or a chained operation is needed. Logs never share stdout.

### Keep a stateful Pi adapter in `e-pi`

The adapter owns monotonically increasing local timeline sequence numbers, current session/model/thinking metadata, in-progress streaming blocks/tool calls, pending deferred-new prompts, and pending Extension UI methods. It maps Pi records to one or more `AgentEvent` values and maps `AgentRequest` values to RPC commands. Authoritative `message_end`, `get_messages`, and `get_state` records repair transient delta state.

Startup sends `get_state`, `get_messages`, `get_commands`, `get_available_models`, and `get_available_thinking_levels`. Session replacement repeats the authoritative state/messages/catalog requests before normal interaction continues.

### Reuse native Pi session storage without becoming a writer

RPC owns all session creation, switching, and writes. Pi RPC 0.84.x does not expose a session-list command, so `e-pi` reads only bounded session metadata from native JSONL files for the Resume roster: the header cwd/id/timestamp, latest `session_info` name, and first user text fallback. Selecting a row sends its absolute file path to RPC `switch_session`. Unknown/new entry fields are ignored, and malformed files are skipped with diagnostics. This read-only exception avoids implementing SessionManager semantics in Rust.

### Map existing frontend requests conservatively

- `Input` -> `prompt`; while Pi is streaming, the adapter uses `streamingBehavior: "steer"`.
- `NewInput` -> correlated `new_session`, then `prompt` only after successful replacement.
- `Interrupt` -> `abort`.
- `Attach` -> `switch_session` using the native path stored in the roster or supplied explicitly.
- `ModelGet` -> model/state/thinking queries.
- `ModelSet` -> `set_model`, followed by `set_thinking_level` when requested, then refresh queries.
- `Command` -> Pi prompt expansion for discovered extension/prompt/skill commands; `/compact` maps directly to `compact`.
- Unsupported DSH-specific or credential-write requests return visible, actionable errors rather than being silently ignored.

### Normalize Pi events into existing display facts

Pi user/assistant/tool messages become `TimelineRecord` facts. Streaming `text_delta` and `thinking_delta` become `AssistantChunk`; authoritative assistant completion becomes `AssistantMessage`. Tool start/end records become `ToolCall`/`ToolResult`; known Pi tools receive provider-neutral capabilities and structured Preview seeds, while extension tools use the generic renderer. Retry and compaction records use existing lifecycle facts. `agent_start`/`agent_settled` control frontend status.

### Implement RPC Extension UI through provider-neutral interactions

`select`, `input`, and `editor` requests open the existing Question Input Page; `confirm` uses the existing approval interaction. Responses are converted to `extension_ui_response` with the original RPC id and method semantics. `notify` becomes a visible notice/error. `set_editor_text` updates the composer through a new provider-neutral interaction event. String widgets/status/title are retained only where the current frontend has a stable surface; unsupported Pi-TUI-only methods follow RPC's documented degraded behavior and are not simulated.

### Transitional reuse of executable-side runtime infrastructure

For this first vertical slice, `e-pi` depends on `e-tui` directly and on `e-dsh`'s library for the existing terminal owner, event loop controller, rendering reducer facade, clipboard/config effect patterns, and Windows VT input implementation. It does not import or instantiate DSH protocol, WebSocket, launcher, setup, or bridge behavior. This avoids copying several thousand lines while the existing client migration is incomplete. A later extraction can move these generic pieces to a shared composition crate without changing Pi RPC or `e-tui` contracts.

## Risks / Trade-offs

- **RPC surface changes across Pi releases** → Validate the runtime version range, keep fixtures from the documented 0.84.x protocol, and fail on unknown required response shapes while ignoring unknown events.
- **`pi` is absent or auth is missing** → Perform an early spawn/version check and show commands to install Pi or run native `pi /login`.
- **Read-only session indexing drifts from SessionManager** → Parse only stable documented fields, bound file/line work, skip malformed entries, and leave all mutations to RPC.
- **Deferred new session is two RPC commands rather than one atomic wire operation** → Correlate `new_session`; send the retained first prompt only after success, and restore it on failure.
- **Extension UI cannot reach Pi TUI parity** → Advertise RPC-mode compatibility, implement documented dialogs/fire-and-forget text surfaces, and reject or degrade TUI-only components.
- **Transitional `e-pi -> e-dsh` dependency weakens adapter isolation** → Architecture tests forbid Pi wire names in `e-tui` and forbid `e-pi` from using DSH protocol/bridge modules; plan a later shared-runtime extraction instead of copying code.
- **Child stdout backpressure** → Use bounded channels and the existing count/time-budgeted event loop; coalesce redraws while retaining every semantic delta.

## Migration Plan

1. Add OpenSpec contracts and the `e-pi` workspace package without changing the default `dshe` member.
2. Implement and fixture-test RPC framing, request translation, event normalization, and session metadata indexing.
3. Add the `pie` composition root and smoke it against the installed Pi 0.84.x CLI.
4. Add architecture checks and concise user-facing install/run guidance only where the new executable changes public workflow.
5. Roll back by removing `crates/e-pi` from workspace membership; existing `dshe` behavior and persisted Pi state are unaffected.

## Open Questions

- Whether a later binary distribution should bundle Node/Pi or install an exact Pi package during a `pie setup` flow.
- Whether missing future RPC operations justify a thin SDK helper, or should wait for the official RPC/protocol surface.
- Whether generic terminal/runtime infrastructure should move from `e-dsh` into a new shared crate before the next adapter is added.
