# Interaction and session contracts

## Input ownership

- Input, page, Reading, approval, search/completion, and link-selection contexts MUST resolve semantic actions before text insertion. Disabled child actions MUST NOT fall through to parent behavior.
- Composer cursor positions are character indices; byte slicing MUST use explicit character-to-byte conversion. Atomic paste/image blocks MUST be skipped or removed as units and MUST expand losslessly on submission.
- Input Pages MUST use one closed frontend-owned session/controller and return effects for execution after state borrows are released. Text-edit states treat ordinary letters as text.
- Help is a blocking input context. It owns close and inherited full-screen scrolling actions while visible; reopening resets its independent scroll position without changing the composer or underlying page.
- Mouse wheel routes to the pane under the pointer. While Help is visible, the wheel scrolls the modal; otherwise separator capture takes priority over text selection.

## Prompt queues

- Pending prompts preserve FIFO within ASAP and after-turn classes; ASAP candidates have dispatch/display priority.
- Admission and clear operations MUST be serialized per session. Their results MUST carry authoritative queue state and session identity; stale-session results MUST be ignored.
- Cancel removes ASAP candidates before after-turn candidates. A clear barrier prevents repeated cancellation and holds newer submissions until acknowledgment.
- Failed admission remains visible without automatic retry. Failed clear preserves the backend snapshot and reports an error.

## Session lifecycle

- Session replacement MUST clear session-scoped questions, approvals, queues, catalogs, and stale async ownership together.
- A bare interactive `/new` creates a frontend draft. Its first prompt or explicit skill atomically materializes the new session; old-session updates continue reducing but remain hidden from the draft.
- New-session failure restores the draft input. Model/effort selection made during the draft applies to the materialized session.
- Resume results MUST preserve stable identity through progressive updates. Native session reads remain adapter-owned, bounded, read-only, and outside UI locks.

## Commands and model selection

- Built-in commands have one metadata registry. Adapter command catalogs MAY extend it, but built-ins win name collisions.
- Direct commands return typed command results and MUST NOT become model messages.
- Model and effort changes are confirmed by the adapter before dependent prompt admission. Temporary marked prompts strip only the mark, preserve exact queued route, and restore the original model/effort after authoritative idle.
- Compaction overrides remain adapter-owned. Manual temporary selection MUST restore and verify the prior route before releasing dependent requests.
