# Implementation design

## 1. Baseline and goals

Baseline locator: Git `273d48f`, plus the existing uncommitted composer navigation/highlighting changes in `e-tui` and the interaction/presentation specs. Those changes are unrelated and must remain untouched. The only existing active change found during planning concerns resume commands, not this goal.

Relevant current entry points:

- [Adapter entry points](../../../../../crates/e-pi/src/adapter/mod.rs): `PiAdapter::request` applies a blanket `pending_compaction` barrier, including `ModelSet` and `ModelGet`; `record` handles manual start/end activity.
- [Compaction transaction](../../../../../crates/e-pi/src/adapter/compaction.rs): one correlated transaction snapshots the conversation route, selects the override, compacts, restores the original model/effort, and verifies. No override currently takes the direct native `Compact` path.
- [Outbound routing](../../../../../crates/e-pi/src/adapter/request.rs): ordinary model changes use native `SetModel` and the existing configuration barrier.
- [Response dispatch](../../../../../crates/e-pi/src/adapter/response.rs): selection proceeds through optional effort application and a correlated state refresh; `drain_deferred` currently requires no compaction transaction at all. Configuration failure can cancel deferred submissions.
- [Model projection](../../../../../crates/e-pi/src/adapter/model.rs) and [session projection](../../../../../crates/e-pi/src/adapter/session.rs): catalog/current-route projection and state refreshes.
- [Existing compaction tests](../../../../../crates/e-pi/src/adapter/compaction_tests.rs): existing cases protect success/failure restoration, cancellation, failed restoration, persistence, and the native no-override path. They do not exercise concurrent model selection.
- [Frontend command routing](../../../../../crates/e-tui/src/runtime/command.rs): the model and effort commands already produce ordinary model requests; no new frontend command or action is needed.

The goal is an adapter-local admission/finalization change, not a new compaction implementation.

## 2. Overall approach

Keep the existing temporary selection and final restoration protocol. Separate three concepts:

1. **Compaction model:** the provider/model captured for this run, immutable after preparation.
2. **Return route:** the last fully confirmed conversation model and effective thinking level. Initially this is the original snapshot; successful user selections replace it.
3. **Admission gates:** preparation/finalization protect all route mutations; running compaction permits model controls but still protects prompts and session/resource mutations.

```text
Abort -> snapshot A -> select compaction model C -> send Compact
  -> native manual start: model captured; model-control lane opens
       select B -> optional effort -> correlated GetState -> return route = B
       select D -> optional effort -> correlated GetState -> return route = D
  -> correlated Compact result: close lane; wait for any in-flight selection
  -> restore return model/effort -> verify return route -> release dependent work
```

Compaction and a user selection can have outstanding RPCs concurrently, but two model mutations cannot overlap. Final restoration cannot overlap a user selection. With no successful user selection, the old A-restoration behavior and order remain unchanged.

### Native handoff and compatibility

The inspected native package is @earendil-works/pi-coding-agent 0.85.1 (installed outside this repository). In its `AgentSession` implementation, `compact()` emits the manual `compaction_start` event, then evaluates `this.model` as the argument to `_getSummarizationRequestAuth` in the same synchronous continuation, before yielding to auth resolution. A later stdin `set_model` callback therefore cannot replace that captured model. The default summarizer uses the resulting `requestModel`. RPC stdin dispatch starts each command independently, and `setModel` has no compaction-completion wait.

Use the observed manual-start handoff, not merely writing `Compact` to stdin, to open the control lane. Match it only to the single adapter-owned compaction awaiting start in the same attached session. Automatic starts, duplicate starts, and unrelated events cannot open the lane. A terminal Compact response without a start still follows the existing safe restoration path; no speculative timeout opens the lane.

This is a compatibility assumption to verify against the supported Pi runtime during implementation, not a new Pi wire field or a guarantee for arbitrary extensions. If the native capture ordering differs, retain the safe barrier and report the compatibility blocker rather than inventing a signal. Extensions supplying custom summaries retain their native semantics.

The handoff captures the **model**, not every request option. `_runDefaultCompaction` reads `this.thinkingLevel` after auth and extension preflight. A model change can affect native preflight thinking before the summary request is constructed, as it already can without an override. Do not claim an immutable compaction effort or change native summarization to manufacture one. Once a request is constructed, a later model selection does not retarget it.

## 3. APIs and data model

All additions are private to `e-pi`; exact Rust names are implementation discretion. Existing public `AgentRequest`, `AgentEvent`, RPC DTOs, and `compaction-model.json` remain unchanged.

Extend the adapter-owned compaction transaction to represent:

- The original confirmed model object and thinking level, retained for fallback/diagnosis.
- The immutable compaction model and activity label.
- The mutable confirmed return route, initialized from the original snapshot.
- Session ownership and a manual-start/controls-open flag.
- The outstanding compact RPC identity and a latched terminal result, distinct from the subsequent restoration RPC identity.
- Whether a normal user model-selection transaction is in flight, including correlation across model, optional effort, and state-confirmation steps. This observation does not become a second writer queue or replace `configuration_request`.

Planned private seams, or equivalent helpers:

- `on_manual_start(adapter: &mut PiAdapter) -> AdapterOutput`: latch the handoff and drain newly eligible controls.
- `on_model_change_confirmed(adapter: &mut PiAdapter, confirmed_route: ...)`: replace the return route only for the matching explicit user selection and authoritative state response.
- `advance_finalization(adapter: &mut PiAdapter) -> AdapterOutput`: consume a latched compact outcome only when no user route mutation is in flight.
- An admission classifier shared by incoming requests and deferred draining, plus a confirmed-route view used by model catalog projection.

Track the purpose and original identity of a model-selection transaction through `SetModel`, optional `SetThinkingLevel`, and `GetState`; the current `configuration_request` ID alone changes between those steps. Here a user/ordinary selection means an admitted `AgentRequest::ModelSet`, including controller-generated temporary-model routing under the existing frontend policy. Do not invent an origin flag absent from the shared API. Internal compaction selection/restoration RPCs, pings, model listing, and arbitrary state refreshes must not be mistaken for a newer ordinary selection. Validate provider/model against the requested target and explicit effort when supplied; otherwise record the actual supported effort returned by Pi.

Authoritative confirmation includes a valid session identity and complete model/effort values. A malformed, mismatched, stale, or failed response cannot promote the return route. Unknown outcomes remain unsafe for dependent prompt admission.

## 4. Algorithms and rules

### Admission and bounded deferred work

Retain the other existing barriers: configuration changes, backend queue operations, and skill prompt admission. This change relaxes only the compaction-specific barrier.

| Compaction phase | ModelGet | ModelSet, including effort changes | Other dependent requests |
| --- | --- | --- | --- |
| Abort / snapshot / override selection / awaiting start | Existing short preparation barrier | Hold | Hold |
| Manual start observed; compact result outstanding | Serve cached catalog and confirmed conversation route | Run through the ordinary serialized selection flow | Hold |
| Compact result latched / restoring / verifying | Hold until finalization settles | Hold new selections; finish the one already in flight | Hold |
| Restoration or verification failed | Retain existing fail-closed behavior | Hold | Hold |

`Interrupt` and extension interaction replies retain their existing bypass behavior. Do not remove the compaction transaction while controls are open.

Use the existing bounded deferred queue, not an unbounded parallel queue. During the running phase, select the earliest eligible `ModelGet` or `ModelSet` from the current-session prefix. It may pass held `Input`, `Steer`, `ClearAsap`, or `Ping` entries; leave those entries and their relative order untouched. Stop scanning at the first `Attach`, `NewInput`, or `Command` entry: model controls must not overtake a session/resource-changing command. Treat all `Command` entries conservatively as fences rather than parsing arbitrary backend commands here.

Incoming controls join this same admission path so they cannot leapfrog earlier model controls or a queued fence. Preserve the existing aggregate capacity of 64 deferred requests and its explicit overflow errors; scanning is bounded by that capacity. While other barriers are active, do not bypass them. When compaction no longer exists, use ordinary FIFO draining.

Run this shared drain after the manual-start event as well as after correlated selection and compaction responses. Draining only after the Compact response would reproduce the original delay for selections queued during preparation. No polling or sleeps are needed.

### Model reads and confirmation

During active overridden compaction, a model picker needs the already-loaded catalog and the confirmed conversation route, not a native `GetState` revealing the temporary compaction model. Serve `ModelGet` from those adapter-owned values; do not trigger compaction-long waits or a temporary C selection in the picker.

Keep the currently confirmed return route visible while a switch is pending. The ordinary native selection still performs real `SetModel`/effort/state RPCs; this is not optimistic UI-only switching. On full confirmation, update the return route and emit the normal model catalog so the composer header and effort change before compaction finishes.

While the transaction owns a temporary route, unsolicited/older state or catalog refreshes must not replace the visible confirmed conversation route with a temporary or partially applied route. Preserve unrelated session/title/statistics handling. Keep model selection and state-confirmation correlation at the response boundary rather than teaching `e-tui` about native compaction states.

### Compact result versus a model selection

On the correlated Compact response, latch its success/error once and close admission of new selections. If a selection is in flight, retain the compaction transaction and wait for that selection's terminal outcome. Do not send restoration writes yet.

On selection success, promote its actual confirmed tuple before advancing finalization. On explicit selection failure, keep the previous confirmed return route, report the error, and retain the existing deferred-work cancellation policy; cancellation feedback must identify that waiting operations were cancelled, never suggest their selections succeeded. An already confirmed earlier selection remains the return route. Partial native mutation is reconciled by restoring that route before dependent work is released. A later selection admitted while compaction is still running can also establish a new confirmed tuple.

If a transport loss leaves the selection outcome unknown, do not assume the old or requested route is active and do not release dependent work; use existing disconnect/fail-closed handling.

### Restoration and final verification

Reuse the current restore-model -> restore-effort -> GetState verification sequence, but target the confirmed return route instead of always targeting the original snapshot. Preserve exact confirmed effort; do not apply a stored default again during restoration. Even if the model is already selected, this conservative reconciliation keeps the existing no-selection path and partial-failure recovery uniform.

New model controls wait during this short finalization sequence. There is consequently no later successful user selection for an old restoration write to overwrite. After verification, clear the transaction and drain requests in ordinary order; a selection queued during finalization then applies normally.

A restoration RPC error, verification mismatch, or malformed confirmation keeps dependent work held and reports the existing actionable recovery guidance. Never release work just to make the UI look responsive.

### Cancellation, activity, and session lifetime

- Interrupt marks cancellation and sends native Abort without waiting behind either lane. Still await the correlated Compact terminal response, not just `compaction_end`, before finalization.
- If cancellation occurs during preparation, preserve existing cancellation/restoration behavior. If it occurs after a confirmed switch, retain that switch as the return route.
- Compaction start/end labels use the captured C label even after conversation selection changes. Do not derive the final activity label from `current_model`.
- Model confirmations and their state refreshes do not acknowledge or clear the interruptible compaction command. Keep command completion and model-control completion distinct; do not release prompts from a transient `isStreaming: false` snapshot.
- Session replacement remains deferred. If the native runtime independently changes session or disconnects, invalidate ownership/correlation before applying further selection or restoration replies; never restore the old session's route into a replacement session.
- Automatic compaction and manual compaction without an override do not acquire this new transaction/lane. Their existing behavior remains unchanged.

## 5. Fixed decisions and discretion

Fixed: adapter-local implementation; native capture handoff before concurrent control; immutable compaction provider/model; confirmed return tuple rather than latest click; serialized user mutations; finalization joins any in-flight selection; existing restoration/verification retained; bounded admission with session fences; no prompt concurrency; no new persistent or cross-boundary ABI.

Rejected alternatives:

- Removing `ModelSet` from the blanket barrier without changing restoration ownership: would overwrite the user's new choice at the end.
- Updating only the displayed model or queuing it until the end: does not fix the reported behavior.
- Restoring the original conversation route immediately after start: unnecessary for the requested interaction, changes the no-selection restoration sequence, and adds another concurrent restoration operation.
- Replacing native compaction with an extension-owned summarizer: substantially expands scope and compatibility obligations.

Private type/helper names and small module extraction are local discretion. The admission rules, confirmation boundary, finalization ordering, and native-effort limitation are not discretionary. There are no unresolved product choices for this plan; native compatibility is an implementation verification gate.

## 6. Verification and documentation impact

Do not add, expand, or modify tests under this request. Run existing scoped suites and a build during implementation:

- `cargo test -p e-pi --lib adapter::compaction_tests::`
- `cargo test -p e-pi --lib adapter::` (adapter-only regression scope)
- `cargo build -p e-pi --bin pie`
- Formatting checks for touched Rust files and `git diff --check`.

Use a disposable session for manual verification, without modifying the user's real configuration. A real provider compaction may incur cost; use an authorized diagnostic session and stop if credentials or service access are unavailable. Confirm B is selected while C compaction is still active; verify the terminal result retains B. Repeat cancellation, compaction failure, rapid successful selections, rejected selection, an in-flight selection at compaction completion, a prompt queued before a switch, and the no-switch/no-override baselines. Do not claim interleavings were exercised unless observed. Report unexercised races as validation limitations; request separate authorization if automated regression cases are needed.

The existing compaction suite passed all seven tests during the preceding investigation. That is baseline evidence only, not validation of this planned change. Existing tests do not prove the new concurrency behavior. If implementation reveals a test that encodes an obsolete interaction, report it and request permission rather than silently rewriting it or distorting production code.

At delivery, update only the manual-compaction interaction clause to describe the fallback/superseding-selection rule and separate control versus prompt gates. Architecture ownership, runtime lock rules, storage format, and presentation-label contracts stay unchanged; no architecture, ADR, README, key-help, generated protocol, or bridge-asset update is currently needed. Preserve the unrelated existing spec edits. During planning, leave all current contracts unchanged.
