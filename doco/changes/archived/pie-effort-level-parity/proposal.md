<!-- doco:change mode=proposal-only -->
# Align pie effort levels with Pi thinking levels

## Purpose

`pie` derives each model's reasoning-effort list from the keys of Pi's
`thinkingLevelMap` in `model_catalog`
([crates/e-pi/src/adapter/model.rs](../../../../crates/e-pi/src/adapter/model.rs)).
Pi does not treat that map as a level catalog. `thinkingLevelMap` maps Pi
thinking-level names to provider-specific effort values and hides levels with
`null`; the selectable set is a fixed ordered list filtered by that map
(`getSupportedThinkingLevels` in the `pi-ai` package, documented in Pi's
custom-provider reference).

Consequences today:

- The `gpt-5.6-sol` model of the `openai-codex` provider
  (`thinkingLevelMap = { xhigh, max, minimal }`) shows
  `Max, Minimal, Xhigh` in pie's **/effort** page, while Pi's selector shows
  `off, minimal, low, medium, high, xhigh, max`. Observed on pie with a live Pi
  0.85.1 child; the order is alphabetical because `serde_json` maps are ordered
  by key, not by level.
- A `null` entry is ignored, so the `openai-codex` provider's `gpt-6-astra`
  (`off: null`) offers `Off`, a level Pi hides.
- A model without `thinkingLevelMap` falls back to `adapter.thinking_levels`,
  which is the **current session model's** supported list returned by
  `get_available_thinking_levels`, so one model's list is applied to every other
  model and changes with session state.
- When the session's level is absent from the declared list, the status bar falls
  back to the raw lowercase id (for example `Effort:medium` on gpt-5.6-sol
  instead of `Effort:Medium`), and no row in **/effort** is marked current.

Model selection and effort selection are user-facing surfaces of `pie`; they must
present the same levels Pi itself accepts for the exact selected route.

## Scope and acceptance

### Deliverables

- Derive `ModelDescriptor.reasoning.efforts` per exact provider/model route from
  the fixed Pi level order `off, minimal, low, medium, high, xhigh, max`:
  exclude a level when the map entry is `null`; keep `xhigh` and `max` only when
  the map contains a non-null entry for them; keep every other level when the
  entry is absent or non-null. Map values are provider-effort translations and
  never become level ids.
- Use Pi level names as effort ids in ascending Pi order, with the existing
  capitalized labels and no descriptions.
- Keep the derived list a pure projection of the model metadata already returned
  by `get_available_models`: no dependence on the current session model, session
  state, or refresh order.
- Remove the now-unused session-level thinking-level query from the adapter if the
  fallback disappears; it is adapter-internal and not part of the DSH bridge
  contract.

### Observable acceptance

1. The `openai-codex` provider's `gpt-5.6-sol` offers
   `Off, Minimal, Low, Medium, High, Xhigh, Max` in that order, with the active
   level marked.
2. The `openai-codex` provider's `gpt-6-astra` (`off: null`) offers
   `Minimal, Low, Medium, High, Xhigh, Max`; `Off` is not offered.
3. A reasoning model without `thinkingLevelMap` offers `Off, Minimal, Low,
   Medium, High`, and its list does not change when the session switches to
   another model or another session.
4. Effort labels resolve against the derived list, so a declared level never
   renders as a raw lowercase id.
5. Non-reasoning models keep `reasoning: None`, so **/effort** stays unavailable
   for them.
6. Catalog contents change only for `efforts`; model ids, names, context windows,
   current selection, and effort-application commands are unaffected.
7. Existing state is not disturbed: no session-file, configuration, execution
   history, or DSH wire change; the adapter still sends Pi-level names to
   `set_thinking_level`.
8. A map that hides every level yields an empty list and the existing
   unavailable state; pie does not fabricate levels.

### Non-goals

- Changing `e-tui` effort-page rendering, focus, marking, or unavailable-state
  behavior; only the adapter-declared list changes.
- Mirroring Pi's `["off"]` list for non-reasoning models; pie keeps its
  no-reasoning gate, where `off` means no effort surface. This difference is
  accepted and out of scope.
- Reading Pi internals, adding RPC commands, or auto-adapting if a future Pi
  version changes its filter rule. The rule is implemented against the observed
  Pi 0.85.1 behavior.
- Changing `default_effort`, which remains the session's current level used for
  label resolution.
- Per-model default thinking levels, model-scoped effort overrides, or level
  cycling behavior.

### Intended contract changes

Only the adapter's projection changes. The `ModelReasoningInfo` shape in the
[wire protocol](../../../specs/wire-protocol.md) keeps its fields and meaning, and
no current document states the level-derivation rule, so no current-document
update is expected. If implementation concludes the rule deserves a durable
statement, the adapter-owned effort projection in
[runtime and adapters](../../../specs/runtime-and-adapters.md) is the place for it.

### Validation sketch

- Scoped unit tests in the `e-pi` crate covering the opt-in map, the
  `null`-hidden level, and the absent-map fallback.
- One live check with `pie` against a Pi child: **/effort** for a model with an
  opt-in map and for a model hiding `off`.

## Result

Delivered and verified for the approved scope.

`model_catalog` now derives each reasoning model's efforts from Pi's fixed level
order with Pi's `null`-hidden and `xhigh`/`max` opt-in rules. The session-scoped
`get_available_thinking_levels` fallback is removed together with the adapter
field and the request/response plumbing that kept it alive.

Verification: `cargo test -p e-pi adapter::` (44 passed) covers the opt-in map,
the hidden level, the absent-map fallback, and the all-hidden map, and checks the
startup query list. Live checks with `pie` against a Pi 0.85.1 child: `gpt-5.6-sol`
lists `Off, Minimal, Low, Medium, High, Xhigh, Max` with the active level marked;
`gpt-6-astra` omits `Off`; a custom-provider map (`codemaker/gpt-5.6-terra`)
lists `Off, Minimal, Low, Medium, High, Xhigh`, matching Pi's
`getSupportedThinkingLevels` for the same model. Non-reasoning models keep no
effort surface. The absent-map case had no live model under the local
credentials, so it is verified by the unit test and by Pi's function returning
`off, minimal, low, medium, high` for such a model. No current-document update
was required.
