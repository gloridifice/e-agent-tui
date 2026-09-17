<!-- doco:change mode=proposal-only -->
# Correct historical tool activity state

## Purpose

When Pi restores a session, an assistant message that ended with `error` or
`aborted` can contain tool-call fragments that Pi never executed. The Pi
adapter currently projects every such fragment as a running tool, so historical
rows show spinners and live durations even though no execution result exists.
This change makes replayed tool activity reflect the authoritative history and
prevents abandoned calls from looking active.

## Scope and acceptance

- During Pi message-history projection, tool-call fragments from an assistant
  message with `stopReason: error` or `stopReason: aborted` are projected as
  terminal failed/aborted activity rather than running activity.
- Replayed tool calls that have a durable matching `toolResult` remain settled
  according to that result; normal completed tool-call history is unchanged.
- Aborted/error tool fragments do not start live-duration timing or leave a
  folded historical file group in the running state.
- Keep focused verification scoped to the adapter/projection implementation and
  existing test suite; do not add a new regression-test case for this fix.
- Keep the fix inside the Pi history adapter/projection boundary; do not alter
  Pi execution, the wire protocol, persistence format, or unrelated active
  changes.

No current architecture or public contract document requires an update: this
is an internal correction of normalized history state.

## Result

Implemented and verified for the approved scope. Pi history replay now derives a
terminal activity state from assistant `error`/`aborted` stop reasons, and the
shared tool projection honors terminal activity states without starting live
timers. No new regression test was added per request; existing focused adapter
and projection tests, formatting, and downstream compilation passed. The
change remains active and has not been completed or archived.
