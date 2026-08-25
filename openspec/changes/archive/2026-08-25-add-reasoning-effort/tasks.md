## 1. Wire contract (v6 + reasoning schema)

- [x] 1.1 Bump `protocolVersion` to 6 in `bridge/protocol-contract.json` and add `ModelReasoningEffortInfo`/`ModelReasoningInfo` records, `reasoning` on `ModelInfo`, `reasoningEffort` on `ModelCurrent`, and optional `reasoningEffort` on `model-set`
- [x] 1.2 Run `node tools/sync-protocol-contract.mjs` to regenerate docs, Rust constants/shape JSON, and fixtures; then `node tools/sync-protocol-contract.mjs --check`

## 2. Bridge session-model adapter

- [x] 2.1 Add `bridge/src/session-model.js` exposing `models(apiProxy, sessionId)` and `selectModel(apiProxy, selection, sessionId)` over the injected `apiProxy.sessions`, with `RpcId` minting and `{ ok, value|error }` unwrapping
- [x] 2.2 Open the apiProxy mux under `ctx.inject(['apiProxy'], ...)` where needed instead of an eager `ctx.get('apiProxy')`
- [x] 2.3 Hydrate `modelSelections.get(agent.id).current` from `session.models` on attach/resume before `welcome`, and write the resolved `current` back into `agent.options`

## 3. Bridge model frame and dispatcher

- [x] 3.1 Extend `shapeModelFrame`/`model.js` to carry `reasoning` metadata and `reasoningEffort` on `current`
- [x] 3.2 Make `dispatcher.js` `model-set` async: validate, call `session.selectModel`, and only on success mutate `modelSelections`/`agent.options` and re-send the model frame; on failure send a `model-failed` error and mutate nothing
- [x] 3.3 Reconcile `/new` mirroring in `session.js` to inherit the full `{provider, model, reasoningEffort}` triple
- [x] 3.4 Send the model frame on `llm/adapters-updated` and `settings/document-updated` (guard with current-conn discipline)

## 4. Rust protocol and adapter

- [x] 4.1 Add `reasoning`/`reasoningEffort` DTOs to `crates/e-dsh/src/protocol/messages.rs` and `ModelSet { reasoning_effort }` (optional)
- [x] 4.2 Extend `crates/e-dsh/src/bridge/adapter.rs` normalization for reasoning metadata and the effort field in both directions
- [x] 4.3 Update `crates/e-dsh/src/runtime.rs` `ServerMessage::Model` and `AgentRequest::ModelSet` handling to store/forward `reasoning_effort` and reasoning metadata

## 5. e-tui model/state/action

- [x] 5.1 Add `ReasoningEffort`/`ModelReasoning` types and `reasoning` on `ModelDescriptor`, `reasoning_effort` on `ModelSelection` in `agent/mod.rs`
- [x] 5.2 Add `AgentRequest::ModelSet { reasoning_effort }` and an effort-resolution helper returning the status-bar label and selectable efforts for the exact current route
- [x] 5.3 Add `CommandAction::Effort` and the `/effort` builtin to `command_catalog.rs`; wire it in `runtime_command.rs` to open the Effort page and send `ModelGet`

## 6. Effort Input Page

- [x] 6.1 Add `EffortPage` and `InputPage::Effort` with loading/ready/empty states, stable focus graph, and Enter → `ModelSet { provider, model, reasoning_effort }`
- [x] 6.2 Add `crates/e-tui/src/ui/pages/effort.rs` renderer and register it in `pages/mod.rs`
- [x] 6.3 Update status bar (`ui/status.rs`) to render `Effort:<Label>` after `CH`, same dim style, hidden when no reasoning metadata

## 7. Overlay, docs, tests

- [x] 7.1 Add `/effort` to `ui/overlay.rs` help text and README quick reference
- [x] 7.2 Add/adjust Rust UI-layer tests (status ordering/color, effort page states, model-set payload) and bridge `node:test` coverage (session-model, dispatcher model-set, `/new` triple inheritance, attach hydration)
- [x] 7.3 Update `docs/client.md`, `docs/bridge.md`, `docs/design.md`; run `cargo fmt --check`, scoped `cargo test`, and `cd bridge && npm test`
