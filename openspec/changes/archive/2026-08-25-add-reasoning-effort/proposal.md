## Why

DSH exposes adapter-owned reasoning effort (low/medium/high…) through `session.models` and `session.selectModel`, but the TUI shows no effort and offers no way to change it. Users cannot see which effort the next turn will use, and cannot override the adapter default from the terminal.

## What Changes

- Show the current reasoning effort in the status bar, after the cache-hit rate (`CHxx%`), as `Effort:<Label>`, styled identically to the model and CH entries, omitted entirely when the current model exposes no reasoning metadata.
- Add a `/effort` command opening an Effort Input Page that lists only the adapter-declared efforts for the exact current provider/model and submits the full provider/model/reasoningEffort selection.
- Extend the model selection to a full triple (provider + model + reasoningEffort) and carry reasoning metadata (`reasoning.efforts`, `reasoning.defaultEffort`) through the `model` frame and `model-set` message.
- Route `/model` and `/effort` changes through the DSH `session.models` / `session.selectModel` API and reconcile them with the bridge's existing model-selection adapter so there is a single source of truth.
- **BREAKING**: bump the wire protocol to v6 (new optional fields and `model-set.reasoningEffort`).

## Capabilities

### New Capabilities
- `reasoning-effort`: status-bar effort display and the `/effort` selector for the exact current model's adapter-declared efforts.

### Modified Capabilities
- `host-model-selection-compatibility`: selection becomes a full provider/model/reasoningEffort triple; changes flow through `session.models` / `session.selectModel` and stay reconciled with the installed model-selection adapter.
- `canonical-wire-schema`: protocol v6 with reasoning metadata on model records, `reasoningEffort` on the current selection and `model-set`.
- `input-page`: a new Effort page (loading/ready/empty states, focus graph, Enter submits the full selection).

## Impact

- Bridge: `session-model.js` (new session API adapter), `model.js`, `dispatcher.js`, `index.js`, `session.js`, `host.js`, `protocol-contract.json`, and `bridge/test/*`.
- Rust: `crates/e-dsh/src/protocol/`, `bridge/adapter.rs`, `runtime.rs`; `crates/e-tui` agent/catalog/action/command_catalog/input_page/ui (status, pages, overlay).
- Docs: `docs/client.md`, `docs/bridge.md`, `docs/design.md`, generated `docs/protocol.md`.
