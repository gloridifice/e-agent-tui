# Implementation

## Baseline and goals

Pi 0.85.1 exposes `get_fork_messages`, `fork`, and `clone` in native RPC. [Pi DTOs](../../../../../crates/e-pi/src/protocol.rs) does not yet model them. Adapter command routing currently forwards most slash commands as prompts and refreshes state only. The frontend already supports generic command catalogs, questions, composer replacement, session attachment, and transcript snapshots.

`session_index.rs` discovers files by modification time and loads bounded metadata in small batches on a blocking worker. ResumePage and the resume renderer are flat. SessionSummary and ResumeBatch are used by both adapters; ancestry will be optional sidecar metadata, not Pi fields added to the shared session summary or a change to the DSH wire schema.

Existing uncommitted composer/layout changes in the frontend input renderer, layout renderer, and presentation contract are unrelated and must be preserved.

## Overall approach

Add a dedicated e-pi fork state machine and typed native RPC commands. Track operation kind, source identity, correlated request/choice ID, optional trailing message, selected editor text, and stage. Fork first fetches user entry IDs and uses the generic question page; clone mutates directly. Native success must also have `cancelled: false`. Refresh state, messages, model catalog, and command catalog in order before releasing the operation. Reuse native attachment and snapshot projection. Only then restore editor text or admit the trailing message. Keep the barrier until trailing prompt admission completes.

Reject other mutation requests while the operation is outstanding; extension UI answers and cancellation remain serviceable. Local picker cancellation never reaches native fork. Once native replacement is in flight, cancellation suppresses the trailing prompt but must still reconcile the authoritative result. Ignore late correlated replies after a cancelled local selection. Commands report typed terminal results on success, cancellation, and errors so frontend command execution cannot remain busy. Native entry IDs, not presentation sequence numbers, identify fork points.

## APIs and data model

Expose a provider-neutral parent-ID map and pure deterministic forest ordering in e-tui. Use an iterative algorithm with cycle breaking, parent-first traversal, family recency from input order, and bounded visual indentation. No file paths are interpreted by shared code.

## Algorithms and rules

The Pi index reads bounded native headers outside UI locks, resolves parent paths against enumerated candidate identities without probing parent targets, and orders candidates parent-first before title paging. The loader retains the optional parent map with the index. Deliver it with admitted resume batches via an additional shared controller entry point, leaving existing flat callers intact. Reject metadata from stale page generations together with its batch.

ResumePage retains raw titles and IDs. Rendering adds connectors and a `(fork)` marker without changing the identity sent to Attach. Search matches raw titles/IDs and includes available ancestors. Preserve the selected session ID as batches arrive. Missing or unreadable parents render as roots and still carry the derived-session marker. No persistent ancestry file is introduced.

## Verification and documentation impact

Run existing adapter, session-index, resume-page/controller and rendering tests, build pie, and use an isolated native RPC/TUI smoke exercise without modifying repository tests or real user sessions. For this cross-cutting Rust change, run workspace formatting and Clippy; preserve unrelated changes and report existing failures. Update current session/presentation contracts, the architecture ownership statement, and concise Pi usage guidance. No new interaction keys require help-key changes.

## Fixed decisions and discretion

All core choices above are fixed. Local helper names and file splits are discretionary. No blocking open questions.
