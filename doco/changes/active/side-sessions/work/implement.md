# Implementation design

## Baseline and goals

Baseline: `9baf99c`, initially clean working tree. Current contracts are [runtime](../../../../specs/runtime-and-adapters.md), [interaction](../../../../specs/interaction-and-sessions.md) and [presentation](../../../../specs/presentation.md). The active `pie-session-forks` change overlaps [native fork](../../../../../crates/e-pi/src/adapter/fork.rs); preserve its standalone replacement contract.

At baseline, the [Pi runner](../../../../../crates/e-pi/src/main.rs) owned one process, adapter, authentication manager, history recorder, shell runner, session loader, frontend state and committed presentation. Its select loop combines provider events with terminal lifecycle. The native fork adapter sends native replacement commands to that single process and requires idle. Reusing it against the source would violate this change.

`e-tui::RuntimeState` already owns independent conversation interaction state. The screen composer resolves one conversation rectangle and Preview; input geometry and selection use that same layout. No side-session API exists at baseline.

## Overall approach

1. Add provider-neutral `runtime::sessions::SessionWorkspace<T>` to own at most two runtime slots and a pending creation reservation. The opaque monotonic frontend `SessionId` is independent of native session IDs and survives role swaps. `SessionEnvelope<T>` binds asynchronous work to this identity. Slot payloads remain generic; no transport, filesystem or provider types enter e-tui.
2. Introduce `e-pi::session_runtime::PiSessionRuntime` as the owned pair of a Pi process and its adapter. Use it in the existing runner before multiplexing. Authentication, shell, discovery and history must also be owned per live slot when the runner is multiplexed; do not share mutable adapter state between slots.
3. Add Pi-owned safe snapshot preparation before starting a second writer. Use the public native session APIs on a detached snapshot to create a distinct saved session, never native replacement against the source process, hand-written JSONL mutation, or a second live agent attached to the writable source. Validate the native snapshot behavior and bounds before exposing the command. Capture a complete historical checkpoint, excluding a live partial response/unresolved tool call. Fork choices are captured once and correlated with the pending creation token. New carries source cwd/model/effort explicitly and does not resume the remembered session.
4. Move provider waiting into per-session workers with bounded channels, or equivalently multiplex independently owned slots in the runner. Every request, event and deferred effect is routed by frontend identity; native replacement remains local to its slot. Run queue dispatch, retries, approvals and shell completion for both slots, even when unfocused. Do not wait for one slot's provider operation in the terminal-input path. This explicitly includes process startup, source snapshot/choice loading and clone/fork materialization: an async function awaited inline in the page handler is not sufficient. Keep a cancellable pending owner, select its completion alongside terminal/provider/frame events, and attach only after authoritative initialization.
5. Integrate screen-owned composition with two independent main panes and one focused Preview. Use one terminal/frame scheduler and one atomic frame submission. A frame commits selection for both slots together; no half-frame publication. Per-pane geometry drives wrapping, composer navigation, scroll, selection and hit testing. A modal remains screen-level and is tied to its owning slot.
6. Expose the command only once supported operations are wired end to end. Register metadata/completion once, reject unsupported adapters locally, and update Help. <code>/side</code> is local navigation or an explicit creation effect, never model input or a temporary prompt-macro route.

## APIs and data model

- `SessionWorkspace::new(T)` creates main and focuses it. IDs are allocated with checked increment and never reused during the workspace lifetime.
- `begin_side(SideSessionKind) -> Result<SideCreation, SessionWorkspaceError>` reserves a new identity and captures the current source. At most one side or reservation exists.
- `finish_side(SideCreation, T) -> Result<SessionId, T>` attaches only a matching reservation whose source still exists. Success focuses side; stale completion returns ownership to the caller for shutdown, rather than dropping resources invisibly.
- `cancel_side(SideCreation)` invalidates only the matching reservation. A later creation gets a new ID.
- `focus(SessionId)`, `focus_other()` and `swap()` change frontend identity/ordering only. Swap keeps the focused identity unchanged and requires two attached slots.
- `close(SessionId) -> Result<SessionSlot<T>, SessionWorkspaceError>` removes an explicitly identified slot only when two exist; closing main promotes side. Return the removed runtime for asynchronous shutdown outside UI locks. Busy confirmation happens before this operation and retains the intended target ID even if focus changes.
- `get`/`get_mut` reject retired IDs. Async results cannot attach to a different conversation merely because it occupies the same position. Native session generation checks within each RuntimeState remain in force as well.
- A shared `SideSessionCommand` parser handles only arguments, rejecting `main`, unknown arguments and trailing text. Registry/controller wiring is conditional on the runtime capability; parsing alone does not advertise support.

## Algorithms and rules

Reserve Preview using existing screen layout before splitting the main rectangle. Use a fixed one-column divider and split the remaining columns equally (odd spare column goes to main). Require at least 24 raw columns per conversation at initial admission. This is a local geometry constant, not persisted configuration. During subsequent narrow resizes show only the focused conversation in the available conversation rectangle, keeping both runtimes and Preview policy intact. Preview-only mode remains explicit and does not destroy either conversation.

Creating a side session is transactional: while selecting/preparing, main remains visible and operational. Failure/cancellation leaves source state, focus and layout unchanged, reports a local message, and releases candidate resources. Repeated create requests are rejected. After attachment, a side transport failure is local, not a reason to terminate the healthy main. Global shutdown closes both workers and prints recovery commands for saved sessions after terminal restoration.

Busy close confirms cancellation of only its captured target. Esc/cancel in a question or ordinary run retains its current meaning; only an actual exit outcome becomes close. A lone <code>/side close</code> errors rather than exiting the software. Removing a slot cancels its shell, authentication, deferred effects and backend, without deleting saved history. Shared global config changes must refresh both slots without overwriting independent route selections.

### Preparation implementation

The Pi companion captures a source checkpoint and fixed fork choices without invoking source replacement or waiting for source idle. Native session reads and branch materialization run in a Node Worker with a timeout and heap limit; worker ownership ends with the source session. The adapter correlates internal control results to operation and source identity, with a bounded preparation deadline but no timeout while waiting for a user's fork choice. Internal commands are omitted from discovery.

Prepared results use provider-neutral `SideSessionSeed` values. An existing saved branch supplies an opaque attachment identity. Native empty branches are not persisted yet and return no attachment identity; target initialization must preserve the supplied parent, workspace and route before accepting input, rather than opening a nonexistent provisional path. The source editor is never changed by a fork choice.

Native branch files are first materialized inside the source worker's private companion directory. Native cancellation terminates the writer before removing its staging directory; process teardown also removes that directory. Background Rust work validates this ownership boundary, copies the native-produced file into the native session directory without overwriting existing files, and carries an uncommitted file owner through the bounded event channel and candidate lifecycle. Dropped results, cancellation and failed startup remove that file; successful attachment commits it as saved history. No frontend handler performs filesystem cleanup.

`SessionWorker::start` owns background startup and the process/adapter/authentication lifecycle. The runner retains this owner and selects its bounded identity-tagged event stream alongside terminal and frame events; no startup or RPC await occurs in the input handler. Independent stop signalling bypasses channel backpressure, and dropping an owner requests asynchronous cleanup. Explicit shutdown awaits cleanup, including native launcher descendants. Target startup and authoritative attachment/history/catalog initialization remain distinct phases: a spawned process is not yet a visible side pane. The workspace runner now multiplexes these workers and their independent frontend, shell, discovery and history owners. Candidate readiness requires authoritative attachment, history, commands and models; only then does the matching reservation become visible. Delayed or rejected results retain cleanup ownership rather than targeting the current focus.

## Fixed decisions and discretion

The proposal records approved user behavior. The identities, creation reservation, source non-mutation, per-slot ownership, no busy check for swap, one Preview, and no new test authoring are fixed. Helper names, equivalent worker organization and rendering helper extraction are local choices. Do not weaken checkpoint safety to make native branching easier; if a public native API cannot produce a safe independent snapshot, block snapshot work and document the evidence before choosing a replacement design.

## Verification and documentation impact

Run scoped compilation after ownership changes, existing session/controller tests as relevant, and workspace formatting/Clippy once cross-cutting integration is delivered. Manually exercise cancellation, stale completions, swap during simultaneous work, closing/promoting either slot, preview preservation and narrow resize through an isolated Pi runtime/TUI. No test files are added or modified. Update architecture for delivered ownership; synchronize interaction/presentation/runtime contracts only when corresponding behavior is wired. README is out of scope. Completion and archival require a separate request.
