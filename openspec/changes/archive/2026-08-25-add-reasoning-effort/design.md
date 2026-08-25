## Context

DSH exposes adapter-owned reasoning effort through `session.models` (`current.reasoningEffort`, per-route `reasoning.efforts`/`defaultEffort`) and `session.selectModel`. The bridge already installs a model-selection adapter (`installModelSelection`) and keeps a per-agent `modelSelections` map, but the wire `model` frame and `model-set` message carry only `provider`/`model`. The TUI has a two-line status bar (model + `CH<cache-hit %>`) and a `/model` Input Page, but no effort display or selector.

## Goals / Non-Goals

**Goals:**
- Show the effective effort in the status bar after `CH`, styled identically to model/CH.
- Add a `/effort` Input Page listing only the exact current route's declared efforts.
- Make the selection a full `{provider, model, reasoningEffort?}` triple, carried by `model-set` and reconciled with the installed adapter via `session.models`/`session.selectModel`.
- Bump wire protocol to v6 so peers agree on the new optional fields.

**Non-Goals:**
- A standalone `/api/session.models` HTTP fetch: the bridge calls the injected `apiProxy.sessions` service directly.
- A generic effort picker across providers/models: selection is strictly route-exact.
- Persisting effort independently of model selection.

## Decisions

1. **Session API via the injected `apiProxy`.** The bridge calls `apiProxy.sessions.models({ rpcId, payload: { sessionId } })` and `apiProxy.sessions.selectModel(...)`, opening them under `ctx.inject(['apiProxy'], ...)` as `question.js` already does (not an eager `ctx.get`). `RpcRequest` requires a `rpcId`; responses are `{ rpcId, result: { ok, value|error } }`. This keeps one Host fact source and avoids HTTP.

2. **Single source of truth.** `session.models` returns `current`; the bridge writes it back into `modelSelections.get(agent.id).current` and into `agent.options`. `session.selectModel` success updates the same pair from `result.value.selected`; failure mutates nothing. The `model` frame and `welcome` continue to read `modelSelections`, so status-bar and next-turn selection cannot diverge.

3. **Effort display resolution.** `displayEffort = current.reasoningEffort ?? exactModel.reasoning.defaultEffort`. Label is `Effort:<Name>`; the id is looked up in `reasoning.efforts` and falls back to the raw id. When the route has `reasoning` but no value at all, show `Effort:Default`; when the route has no `reasoning`, hide the entry. Only `catalog`/`session` page state changes — never `TranscriptRenderCache`.

4. **Extend, don't fork, the wire schema.** Add `reasoning` to `ModelInfo`, `reasoningEffort` to `ModelCurrent` and to `model-set`. Reuse `model-get`/`model-set`; no `effort-get`/`effort-set`. Bump to v6.

5. **`/effort` is a new Input Page** (`CommandAction::Effort`, `InputPage::Effort(EffortPage)`), reusing the §4.7 shell, focus graph, loading/empty states, and `ModelGet`/`ModelSet`. `/model` keeps sending `provider+model` (omitting effort, clearing the old model's effort); `/effort` sends the full triple.

## Risks / Trade-offs

- [Two selection waterfalls diverge] → `session.models`/`session.selectModel` result is written back to `modelSelections` on every read/select; the bridge-owned pair remains the only thing the TUI sees.
- [Stale effort after a model switch] → `/model` omits `reasoningEffort`, so the adapter clears it and `session.models` reports the new route's default (or none).
- [Cross-await session leakage] → every async model/effort operation captures `conn` before awaiting and re-checks `conns.isCurrent`/connection identity before mutating or sending.
- [Older embedded bridge silently drops effort] → v6 bump makes a mismatched client/bridge fail the existing version gate instead of degrading silently.
- [Subagent sessions reject selectModel] → `/effort` renders an unavailable state when DSH rejects or the route has no efforts.
