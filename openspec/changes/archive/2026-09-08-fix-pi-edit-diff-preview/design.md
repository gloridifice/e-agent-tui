## Context

Pi RPC emits complete `edit` arguments at `tool_execution_start` and returns both a display diff and a standard unified patch under `result.details` at `tool_execution_end`. The Pi adapter currently discards both structured forms: it gives edit calls a generic JSON seed and emits results with no mutation facts. `e-tui` already renders call-time `MutationHunk` values and event-authored unified diffs, but its result fact currently carries only mutation hunks.

The frontend architecture forbids reading files or computing diffs in the client. All presentation must therefore derive only from the RPC events.

## Goals / Non-Goals

**Goals:**

- Show Pi edit arguments as removed/added mutation rows while the call is pending.
- Replace the pending fragments with Pi's authoritative standard unified patch after success.
- Preserve equivalent behavior for live events, session replay, and result-before-call history reduction.
- Keep edit calls eligible for existing file-activity folding.

**Non-Goals:**

- Reproduce Pi's asynchronous call-time file read and contextual preview computation.
- Read target files, apply replacements, or calculate a diff in Rust.
- Interpret arbitrary extension tools as Pi's built-in edit schema.
- Change the Pi RPC protocol or DSH wire protocol.

## Decisions

1. **Represent call-time Pi edits as `ToolReference::Hunks`.** Each valid `edits[]` entry becomes one `MutationHunk` carrying the event path, `oldText`, and `newText`. Legacy top-level `oldText`/`newText` is also accepted for replay compatibility. This reuses the provider-neutral mutation presentation already used by DSH and avoids a Pi-specific renderer.

2. **Carry result-time unified patches as a distinct normalized mutation fact.** `TimelineFact::ToolResult` and staged result state gain an optional event-authored unified diff value. The Preview reducer prioritizes this value over result hunks, while retaining the call-time reference when neither is present. Keeping unified text intact avoids lossy patch parsing and satisfies the existing verbatim rendering contract.

3. **Use `details.patch`, not `details.diff`.** Pi documents `patch` as the standard unified patch intended for SDK/RPC consumers. `details.diff` is Pi's TUI-oriented numbered display format and would duplicate line-number presentation in `e-tui`.

4. **Recognize single-path hunk references as file activities.** File projection accepts `ToolReference::Hunks` only when its non-empty hunks identify one consistent path. Multi-path or pathless sets remain ordinary tool activities rather than being mislabeled as one file.

5. **Soft-fallback malformed data.** A complete built-in edit schema produces mutation hunks. Incomplete edit arguments retain the existing path or bounded JSON fallback, and absent/non-string result patches leave the call-time Preview unchanged.

## Risks / Trade-offs

- **Pending Preview has no surrounding context or authoritative line coordinates.** → Label it only through raw removed/added fragments and replace it with the result patch when available.
- **Older Pi versions may omit `details.patch`.** → Retain the call-time hunk after settlement.
- **Adding a normalized result field touches both adapters and staged history state.** → Default DSH normalization to no unified diff and cover live plus replay ordering with focused tests.
