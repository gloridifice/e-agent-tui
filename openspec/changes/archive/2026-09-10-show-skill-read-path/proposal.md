## Why

Agent-initiated skill reads currently look like explicit user skill invocations and hide the read path. Show them as ordinary read activities with an accented skill identity.

## What Changes

- Render agent skill reads as `• read [skill] <name> at <path>` with ordinary activity state indicators and spacing; keep Rose/Mist for the skill tag/name and omit metrics.
- Preserve explicit user `[Skill] <name>` cards, Preview paths, correlation, and execution-history classification.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pi-agent-frontend`: agent-initiated skill read presentation.

## Impact

Shared tool projection and transcript rendering, focused regression tests, and the client architecture summary. No adapter transport or persistent-format changes; reuse existing normalized tool references and workspace-relative path formatting.
