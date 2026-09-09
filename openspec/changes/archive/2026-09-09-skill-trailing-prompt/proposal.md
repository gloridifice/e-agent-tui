## Why
Skill commands with trailing text need to submit a skill followed by a separate user prompt in both frontends, rather than dropping the text or passing it only as skill arguments.

## What Changes
- Parse colon and space skill invocations with optional trailing text.
- Admit the skill instructions followed by a separate user message on the same session, including deferred new sessions.
- Preserve existing failure and stale-connection guards; do not send the text if skill lookup fails.

## Capabilities

### New Capabilities
- `openspec/specs/skill-prompts/spec.md`

### Modified Capabilities
None.

## Impact
Bridge skill parsing/dispatch/injection and Pi RPC submission/admission ordering; focused Node and Rust regression tests plus generated release assets. Reviewed `deferred-new-conversation` and `pi-agent-frontend`: atomic materialization and Pi-owned native skill expansion remain unchanged. No wire shape change.
