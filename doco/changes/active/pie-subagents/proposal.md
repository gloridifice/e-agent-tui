<!-- doco:lifecycle v=1 created-at=2026-11-20T10:02:47Z completed-at=- archived-at=- -->
# pie-subagents

## Purpose
Let a Pi conversation delegate a bounded task and continue working, without opening or taking focus from another conversation.

## Scope and acceptance
Default to off, with no model-facing injection. <code>/subagent</code> toggles; <code>/subagent on</code> and <code>/subagent off</code> are idempotent. The footer shows the mode independently of active tasks. The parent chooses a one-word work label and receives a task ID immediately. It may override the model or thinking level for that child without changing its own settings. A temporary child runs in the same workspace and sends one bounded native follow-up to its owning conversation, waking an idle parent. The composer bottom rule shows compact, display-only colored squares and labels. Session replacement must not deliver stale work. No recursive tool, inbox, child transcript, split pane, management commands or resume support.

## Result
Pending — not completed.
