# Implementation design

## Baseline and goals
Pi already loads a public companion for native controls. Keep the existing side conversations independent. Preserve ongoing selection-layout edits in the worktree.

## Overall approach
Only a command switch lives in the existing companion at startup. Load the implementation and register the model-facing tool on explicit enable. The adapter normalizes private status snapshots. DSH does not advertise the capability.

## APIs and data model
<code>/subagent [on|off]</code> toggles or sets per-session capability. No persistent settings. `spawn_agent(label, task)` admits up to three runs. One owner-scoped snapshot has the capability and up to six colored label/status entries. Capture bounded final output, not the event stream.

## Algorithms and rules
Run a fresh `--print --no-session` child with inherited workspace, route and project trust, without the pie companion. Send task text via stdin. Recycle the oldest settled display slot if necessary. On completion use Pi custom follow-up delivery with turn triggering. Invalidate ownership before disable/shutdown cleanup. Hide the tool on disable. Keep the mode visible in the footer without requiring tasks. Allocate the bottom rule among shell/macro, side opening, compact task entries and Tips.

## Fixed decisions and discretion
No recursive delegation, child management, persistence, DSH protocol changes or new bindings. Exact palette and capacity are implementation choices. No blocking decisions.

## Verification and documentation impact
Verify the real companion via offline native RPC, type-check both adapters and exercise existing focused suites. Update runtime, interaction and presentation contracts and architecture.
