<!-- doco:change mode=proposal-only -->
<!-- doco:lifecycle v=1 created-at=2026-11-23T08:48:35Z completed-at=- archived-at=- -->
# btw-questions

## Purpose
Ask about the focused conversation without steering, interrupting or waiting for its current task.

## Scope and acceptance
- <code>/btw &lt;message&gt;</code> asks a one-shot, tool-free question using the selected Pi model and effort and the current branch’s completed model context, including compaction summaries. No queued prompt, partial reply or unanswered tool call belongs to that snapshot.
- Show the question and its answer together in the message pane marked `[BTW]`. Do not open a pane, move focus, append native session entries or persist a side history. Subsequent side questions must not see earlier exchanges.
- Keep delivery independent of main agent/queue state and scoped to the originating conversation. Unsupported backends and unavailable contexts must reject locally rather than forwarding a prompt.

## Result
Pending — not completed.
