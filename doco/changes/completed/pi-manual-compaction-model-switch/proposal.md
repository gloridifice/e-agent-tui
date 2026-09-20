<!-- doco:lifecycle v=1 created-at=2026-09-20T08:40:28Z completed-at=2026-09-20T09:24:39Z archived-at=- -->
# pi-manual-compaction-model-switch

## Purpose

Allow conversation-model selection while Pi manual compaction is running with a configured compaction-model override. Today the adapter temporarily changes the native session model for compaction and holds both model reads and model changes until compaction, restoration, and verification finish. A user selection therefore takes effect only after the long-running operation ends.

The compaction request's captured provider/model and the user's subsequent conversation selection have different lifetimes. They should not share one blanket request barrier. Simply removing that barrier is unsafe: final restoration can overwrite a newer selection, and a switch dispatched too early can change the model used to start compaction.

## Scope and acceptance

### Deliverables

- Keep the active compaction's provider/model fixed, while allowing model-picker reads and ordinary model/effort selections after the native manual-start handoff.
- Retain the original conversation model/effort as the initial return route. Replace that return route only after an explicit selection is authoritatively confirmed, including its effective effort.
- Serialize model changes and compaction finalization. Finalization restores/verifies the latest confirmed return route, not unconditionally the pre-compaction route.
- Continue holding prompts, session replacement, resource reload, and other commands until compaction and route verification settle. Keep cancellation available.
- Preserve truthful presentation: the conversation header follows confirmed conversation selections; the compaction activity retains the captured compaction-model label.

### Observable acceptance

1. With conversation model A and compaction model C, start manual compaction and select B. While compaction is still running, the model change is sent, confirmed, and displayed as B; the compaction activity still names C.
2. After compaction succeeds, fails, or is cancelled, the conversation remains on B and its confirmed effort. If no selection succeeded, restore A and its original effort as today.
3. Repeated successful selections are serialized; finalization preserves the most recently confirmed selection. A rejected or partially completed selection is never advertised as a confirmed return route.
4. If compaction finishes while a model change is in flight, await that change's terminal outcome before issuing restoration writes. No restoration may overwrite a later confirmed selection.
5. A prompt queued before a model selection does not cause that selection to wait for the whole compaction. Prompt ordering and marked-prompt routes remain intact; model controls never overtake a queued session/resource-changing command.
6. There is no premature prompt dispatch, session replacement, duplicate completion, or release after failed restoration/verification.
7. Manual compaction without an override and automatic compaction retain their existing behavior. The no-selection override path retains its existing restoration sequence.

### Intended contract change

On delivery, refine the manual-compaction clause in the [interaction contract](../../../specs/interaction-and-sessions.md): the original route is the fallback, but a newer authoritatively confirmed conversation selection supersedes it. Model controls may run during compaction after model capture; prompt/session-dependent work still requires compaction settlement and verified conversation routing.

The active compaction's captured-selection rule in the [configuration contract](../../../specs/configuration-and-storage.md) remains in force. Planning does not change current specs.

### Non-goals

- Changing the model of an already-running compaction, restarting compaction on a model switch, or enabling simultaneous prompt execution.
- Changing automatic compaction, DSH, native Pi, the companion extension, shared frontend APIs, keys, or persisted configuration formats.
- Introducing a compaction-specific effort setting or promising a new native preflight-effort snapshot boundary. Pi currently captures the model before asynchronously resolving auth, but reads thinking level later when constructing the summary request; retain that native behavior.
- Adding or modifying tests without explicit user authorization.

## Result

Delivered. Pi manual compaction now captures its compaction model before opening a serialized model-control lane. Confirmed model/effort changes during compaction supersede the original route; compaction completion waits for an in-flight selection, then restores and verifies the latest confirmed route before releasing dependent work. Prompt and session mutations remain held, and automatic/no-override paths are unchanged.

Verification: 7 compaction tests and 51 Pi adapter tests passed; `pie` built; scoped Clippy, rustfmt, `git diff --check`, Doco checks, and a disposable RPC interleaving simulation passed. Full dependency Clippy remains blocked by 31 pre-existing `e-tui` warnings; no live paid-provider run was performed.
