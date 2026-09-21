# Pi session forks

## Purpose

Expose native Pi fork and clone operations in pie and show their parent-child relationships in the resume picker, rather than mixing derived sessions into an unrelated flat list.

## Scope and acceptance

- The `fork` slash command with an optional message selects a historical user message and creates a native fork before that entry. Without an argument, restore the selected text to the composer. With an argument, send that exact message only after the replacement session and its history are confirmed.
- The `clone` slash command with an optional message copies the current active branch. Without an argument, leave the composer empty; otherwise send the message after the replacement is confirmed.
- Cancelled or failed operations must not send the trailing message. Admit these operations only while idle; serialize replacement and refresh against other adapter mutations.
- Read native `parentSession` metadata without changing Pi's persistent format. Render derived sessions beneath their parents using tree connectors and a `(fork)` marker, including nested descendants. Native clone and fork share the same ancestry marker because their headers do not distinguish the operation.
- Keep progressive discovery, stable selection, modification ages, search, and exact resume identity. Missing parents remain selectable roots; malformed cycles must not hide sessions or hang the UI. Search retains available ancestors of matches.
- Pi owns session persistence and lifecycle. The shared frontend owns only provider-neutral ancestry ordering and rendering; DSH remains flat when no ancestry is supplied.

## Non-goals

Same-file tree navigation, collapsible trees, new key bindings, Pi changes, manual JSONL rewriting, automatic session renaming, and filesystem/Git rollback.

## Acceptance

Both commands are discoverable in pie. Success, cancellation, failure, and trailing-message sequencing are checked using existing scoped tests, a build, and isolated manual RPC/TUI checks. The resume picker displays parent-first nested trees with search ancestry and preserves selection during incremental loads. No tests are added or modified. Current contracts and user guidance describe the delivered behavior; this change remains active unless completion is separately requested.

## Result

Pending.
