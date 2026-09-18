# Implementation design

## 1. Baseline and goals

The normalized timeline already carries provider-neutral user, assistant,
reasoning-step, tool, turn-end, catalog-model, and token-usage facts. Pi parses
`usage.cost.total`; DSH parses token usage but receives no price. The shared
`ExecutionCapture` currently records only turn and operation lifecycle events,
and both adapters persist those records through parallel JSONL stores. The
history page queries only ranked calls and renders one table.

The goal is to enrich the existing capture boundary rather than parse provider
payloads in the history UI. Existing uncommitted work touches nearby files; all
edits must be targeted and preserve unrelated changes.

## 2. Overall approach

Pi converts native response cost into a non-surface normalized usage-cost fact
immediately before the matching final assistant fact. `ExecutionCapture` owns
current model identity, pending response cost, and current turn identity. It
combines the pending cost with the next final assistant usage sample, records
message kinds from normalized lifecycle events, and never persists content.
DSH follows the same capture path but supplies no usage-cost fact.

Both adapter stores continue using the common `ExecutionRecord` wire type and
append-only version-1 JSONL envelope. Their query loops collect records at the
same EOF watermark used for ranking and return both products in one reply.

`HistoryPage` owns loaded chronological records, ranking rows, a view enum, and
independent offsets. The history region keeps the existing ranking renderer and
adds a timeline document/render path. Timeline rows are generated from event
timestamps on a fixed five-second grid; elapsed gaps therefore remain visible.

## 3. APIs and data model

Shared execution-history additions:

- `ModelIdentity { provider, model }`.
- `HistoryMessageKind`: user, reasoning, assistant, tool call, tool result,
  model change, and agent stop.
- `TokenUsageRecord`: input, output, cache-read, and cache-write `u64` counts.
- `ExecutionEvent::ModelSelected { model }`.
- `ExecutionEvent::MessageObserved { turn_id, kind, model }`.
- `ExecutionEvent::UsageRecorded { turn_id, model, usage,
  cost_usd_nanos: Option<u64> }`.

USD is stored as integer nanodollars to avoid JSON floating-point accumulation
drift. Existing records and headers remain valid because the new externally
tagged enum variants are additive.

For the longest-50 page query, both stores now retain the chronological
`HistoryQueryResult.records` that were previously discarded after ranking. They
collect those records only through the query watermark; appends that race after
the watermark are excluded from both ranking and timeline results.

`TimelineFact::UsageCost` is provider-neutral, hidden from transcript
projection, and consumed only by execution capture. Recorders seed capture with
the attached session's optional model route, and capture observes subsequent
normalized catalog changes.

## 4. Algorithms and rules

A normalized turn-start reserves the capture-local turn identity when supplied;
otherwise the first user message creates it. The user event defines the visible
and statistical start of the turn. Subsequent events carry that id until turn
end emits agent stop and closes it. Model changes update capture state without
changing turn. Assistant usage is charged exactly once when the final assistant
message arrives; a preceding usage chunk may supply the pending sample, but
streaming chunks and tool/message events do not duplicate token or cost data.

For every subtotal, tokens are saturating sums of all four token classes and
price is the sum of known native amounts. If any token-bearing usage sample has
no price, a non-zero known sum is rendered with an unknown suffix and a zero
known sum is rendered as unknown. Empty usage is not treated as priced usage.

Timeline origin is the earliest displayable message/model/operation event
rounded down to a five-second boundary. Multiple events in one cell are rendered
as compact inline labels without introducing a selection cursor. Agent-stop rows
display the enclosing turn's aggregate tokens and price. Open turns have no stop
total. Legacy histories can still display their existing operation lifecycle;
if they contain no displayable event, the timeline renders an unobtrusive
unavailable message. Their ranking remains unchanged.

Store read errors, malformed JSONL, overlong lines, and worker failures retain
existing behavior. No provider text or raw payload is persisted.

## 5. Fixed decisions and discretion

Fixed by the request and approved prototype: ranking remains available; `Tab`
switches views; five seconds per row; summaries scroll; Ferra semantic colors;
no timeline chrome, cursor, sticky section, or duplicated message-block
charging.

Implementation discretion: exact spacing and truncation under narrow terminals,
message abbreviations, and use of existing semantic theme roles. Price precision
shown by the UI may be rounded, but persisted nanodollars and aggregation are
not converted through floats after capture.

## 6. Verification and documentation impact

Run formatting checks on touched Rust files, focused `e-tui`, `e-pi`, and
`e-dsh` checks/tests that already exist, Doco validation, and a headless TUI
smoke inspection if the binaries can be exercised without disturbing the
workspace. Do not add or modify tests under the repository policy; report any
existing assertion that still encodes the retired history-Tab behavior.

Update [configuration and storage](../../../../specs/configuration-and-storage.md),
[interaction and sessions](../../../../specs/interaction-and-sessions.md), and
[presentation](../../../../specs/presentation.md).
No architecture boundary or ADR changes are expected because adapter-owned
storage and normalized-event ownership remain unchanged.
