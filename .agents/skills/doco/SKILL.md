---
name: doco
description: Manage project documentation and doco changes. Use when creating, executing, reviewing, completing, or archiving a doco change, or maintaining the project's current technical documents.
---
<!-- doco:managed template=v1 -->

# Doco

Start from the project's `doco/architecture.md` and read only relevant current
specs, effective decisions, source code, and the selected change.
For documentation-only work, follow these current-document rules without
creating a change unless the user requests change tracking.

Read the reference for the requested action:
- Create: `references/create.md`
- Execute: `references/execute.md`
- Review or complete: `references/complete.md`
- Archive: `references/archive.md`

Do only the requested phase. Do not create a change for discussion alone.
Resolve relative reference paths from this skill directory.

Current architecture describes implemented boundaries, not future plans.
Specs are approved precise contracts, not copies of internal code. Investigate
code/spec disagreements; never rewrite a spec just to hide an implementation bug.
Keep only important durable rationale in decisions; label superseded decisions.
Historical proposals and superseded decisions cannot override current contracts.
Keep project document language consistent with the user or project convention.
Installation grants no permission to execute code, complete, archive, or cancel.
