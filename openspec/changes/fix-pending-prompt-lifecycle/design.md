## Context

Local input-before-dispatch ordering is implemented. The user has now approved batch cancellation (option 1): Escape cancels all pending ASAP messages first, leaves local after-turn messages intact, then cancels after-turn messages newest-first only when no ASAP work remains.

## Goals / Non-Goals

**Goals:** Preserve visible pending messages through backend admission, cancel ASAP without aborting active work, and preserve FIFO delivery and local after-turn drafts.

**Non-Goals:** Selective backend cancellation, clear-and-requeue, replacing the official Pi RPC child, or patching global dependencies.

## Decisions

- Keep local candidates, one admission-in-flight candidate, and an authoritative backend queue snapshot in the frontend queue owner. Render their combined projection with ASAP above after-turn messages. No text equality matching or per-message cancellation identities are needed for batch cancellation, including duplicate/expanded prompts.
- Serialize frontend ASAP admissions until acknowledgment. Adapters buffer queue snapshots during admission and include the latest snapshot in the acknowledgment, avoiding duplicate optimistic/backend rows.
- Escape removes local ASAP candidates and establishes a clear barrier. If admission is in flight, wait for its acknowledgment before issuing clear; suppress repeated Escape until clear completes. Messages typed after the cancellation stay local until the barrier completes and survive it.
- Backend snapshots retire consumed candidates; authoritative user events alone enter the transcript for steering prompts. Normal idle submissions retain immediate feedback.
- DSH wire carries queue snapshots and submit/clear acknowledgments with session identity. A dedicated bridge module owns inbox observation and cancellation. The pinned dsh-agent 0.1.1-rc.2 tarball was verified to expose nextStep, splice, and inserted/claimed/discarded notifications.
- Pi uses official queue_update and clear_queue. Admission uses prompt with streamingBehavior=steer so a late idle race still starts work. Queue updates are buffered until the correlated prompt response, and clear waits for admission preflight. Pi clear_queue also clears extension-origin steering/follow-up messages; this batch scope is explicitly accepted by the user. No survivors are requeued.
- Session changes discard queue state and invalidate old acknowledgments. Failures release operation barriers, retain authoritative remote candidates, and surface errors rather than interrupting work.

## Risks / Trade-offs

- [Consumption races clear] -> Apply the latest authoritative snapshot and never reinterpret that Escape as interrupt.
- [Pi extensions transform or consume input] -> Use queue snapshots, not input text matching.
- [Failed admission] -> Preserve the complete prompt as a non-dispatching failed candidate until cancellation, without overwriting the user's current composer draft.
- [Old Pi lacks queue_update or clear_queue] -> Document the required RPC capabilities and report command failures without claiming cancellation succeeded.
- [Other producers enqueue after clear] -> Later snapshots represent fresh candidates; a clear acknowledgment does not permanently suppress updates.

## Migration Plan

Bump and regenerate the DSH wire contract, rebuild clients, deploy with dshe setup, and restart DSH. Pi retains the official RPC launcher. Update help and README for the revised cancellation behavior.
