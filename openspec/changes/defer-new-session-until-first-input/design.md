## Context

The bridge currently creates a DSH agent/session during `/new`, mounts preset state, claims the workspace, and immediately reattaches the socket. Preset setup appends permission/sandbox/approval events, which materialize a JSONL log even when no model turn ever starts. The Rust client has one authoritative `AppState`/`TranscriptStore`, while the socket remains session-scoped and inbound event frames do not carry a separate session ID.

DSH 0.1.0-rc.6 already defines list-level blankness as “no `turn/start` event” and exposes the `sessionListMetadata.blank` projection through live and persisted projection services.

## Goals / Non-Goals

**Goals:**
- Make `/new` instantaneous and local until the first ordinary prompt.
- Keep the old attached session authoritative and up to date behind the draft.
- Atomically address the first prompt to a newly created bridge session.
- Hide every no-turn session from `/resume`, including existing setup-only logs.
- Preserve typed controller/effect and canonical protocol boundaries.

**Non-Goals:**
- Delete persistence artifacts or workspace ledger entries through private filesystem APIs.
- Make a fresh unaffiliated `hello`; startup may still create a real blank session and relies on blank-history filtering.
- Support session-scoped `/model`, `/skill`, or integrated commands before the draft is materialized; these actions report that a first prompt is required.
- Replace DSH’s blank definition with title presence or human-source heuristics.

## Decisions

### Keep a draft marker in client application state without replacing the old transcript

`AppState` gains typed pending-new metadata (mode and materialization phase/input), but retains the old session ID and transcript. Rendering and copy layout treat the draft as a blank presentation and status/title use draft overrides. Inbound frames continue reducing into the hidden old session, avoiding unaddressed-frame corruption. A real `welcome` for a different session commits the switch through the existing reset path and clears the draft.

Alternative considered: synthesize a local `welcome`. Rejected because it would invent identity, persist the wrong last-session ID, discard the old transcript, and make old session-scoped frames indistinguishable.

### Add one atomic `new-input` client message

The first prompt sends `{type:"new-input", mode, text}`. The dispatcher creates and attaches the new session, updates its connection closure, then calls `followup` with the typed user message. `/new` itself sends nothing.

Alternative considered: send `/new`, wait for `welcome`, then send `input`. Rejected because welcome correlation, creation failure, disconnect, and rapid repeated-new races become client protocol state.

### Use `turn/start` as the durable history eligibility boundary

A session enters `/resume` only after one model-loop execution starts. Live sessions fold their memory events. Cold sessions prefer `sessionProjectionCache` (`sessionListMetadata.blank`), refresh through `coldSnapshot` when needed, and fall back to `persistence.readFrom(id, 0)`. Classification is batched, fail-open on operational errors, and happens before the 200-row cap so blanks cannot crowd out real history. Title enrichment remains progressive after eligibility is known.

Alternative considered: filter empty titles. Rejected because title generation can fail and explicit titles do not prove a model turn.

### Draft failure remains retryable

The first prompt is retained in pending-new state while materialization is in flight. A `new-failed` error restores it to the input editor and returns the draft to ready. A successful different-session `welcome` clears the draft; the normal user event then supplies transcript content.

### “新对话” is display-only

The draft title is rendered as `新对话`; no `session/title` is appended. Existing real untitled-session fallback remains independent.

## Risks / Trade-offs

- [Startup still creates a real blank session] → Server-side blank filtering hides it; a future unaffiliated hello can remove the artifact without coupling this change to authentication redesign.
- [Old session receives background events while draft is visible] → Continue reducing them into the retained store; block `/new` when a blocking question/approval exists and avoid routing draft input through the old queue.
- [Projection services may be absent after a DSH composition change] → Fall back to typed persistence reads and fail open on errors; cover deployed compatibility.
- [Crash between session creation and first followup can still leave a blank artifact] → The same blank filter removes it from history.
- [Protocol version changes] → Update only the canonical contract and regenerate all derived metadata/docs/fixtures.

## Migration Plan

1. Deploy synchronized bridge/client protocol version and restart the dedicated `dshe` profile.
2. Existing blank logs are hidden on the first `/resume` request; no data migration is required.
3. Rollback restores eager `/new`; newer clients are rejected by older bridges through protocol negotiation rather than misrouting `new-input`.
