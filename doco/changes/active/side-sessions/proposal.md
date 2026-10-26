<!-- doco:lifecycle v=1 created-at=2026-10-26T08:48:02Z completed-at=- archived-at=- -->
# Side sessions

## Purpose

Allow a user to continue a separate conversation alongside an existing working conversation without hiding Preview or replacing the original runtime. Main and side are frontend roles, not backend session identities.

## Scope and acceptance

- Deliver Pi support first through provider-neutral frontend interfaces. DSH retains its existing single-session behavior and must not advertise unsupported creation operations.
- Permit one main and one side conversation. Each owns its transcript, composer draft, scroll, Preview selection, model/effort, queues, questions, approvals and runtime. Both can work concurrently in the same workspace; this does not isolate filesystem changes or create Git worktrees.
- Keep the existing Preview visibility and width. Divide the remaining conversation area equally between main (left) and side (right). Share the visible Preview surface, following focus and restoring each conversation's Preview selection/scroll. Mouse clicks focus a conversation; wheel scrolling does not change keyboard focus.
- Bare <code>/side</code> clones and focuses a side conversation when absent, otherwise switches focus. <code>/side clone</code>, <code>/side new</code>, and <code>/side fork</code> create and focus a side conversation; reject creation if a side conversation or creation operation already exists. Fork displays a fixed snapshot of historical user entries; cancellation leaves the original layout and runtime intact.
- Side-runtime startup, clone snapshot loading, fork-choice preparation and branch creation MUST run asynchronously without blocking the current page. While creation is pending, input, scrolling, focus changes and both existing conversation/event streams remain responsive. Cancellation or stale completion MUST release candidate resources; only an initialized candidate may attach and receive focus.
- Clone and fork may read a safe historical snapshot while the source continues working. Never copy an active task, partial response, queue, approval, question, or composer draft. Fork restores the selected user text in the new composer; clone leaves it empty. New inherits workspace, model and reasoning effort but no history.
- <code>/side close</code> and ordinary exit commands close the focused conversation while two are open. Closing main promotes side without exiting the program. Closing a busy conversation requires confirmation and stops only that runtime. Saved history is retained. With no side, <code>/side close</code> reports that no side session is open; ordinary exit behavior is unchanged.
- <code>/side swap</code> exchanges positions and roles without backend requests or idle restrictions. Focus follows the conversation. <code>/side main</code> does not exist.
- Reject opening a side conversation when the existing Preview allocation leaves insufficient width for two usable composers. If an already-open split becomes too narrow, retain both runtimes and display the focused conversation until sufficient width returns; do not hide Preview to accommodate the split.
- No new or modified tests without separate user authorization. Validate using compilation, scoped existing checks and isolated manual runtime exercises. Update help and current contracts as behavior is delivered, not before.

## Non-goals

More than two conversations, DSH side-runtime implementation, automatic session deletion, workspace isolation, new persistent frontend session formats, changes to native standalone <code>/fork</code> and <code>/clone</code>, and automatic completion or archival of this change.

## Result

Pending — implementation in progress, not completed.
