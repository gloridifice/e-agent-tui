## Why

Agent-initiated skill reads in pie look like ordinary file reads instead of identifying the skill being loaded.

## What Changes

- Recognize Pi read calls whose target basename is exactly `SKILL.md`, supporting both path separators.
- Display a compact `[Skill] <parent-directory>` activity while retaining tool correlation, result state, Preview, and history replay. Bare `SKILL.md` uses the session cwd basename, falling back to `SKILL.md` when unavailable.
- Leave actual reads and explicit skill commands unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pi-agent-frontend`: recognize and present agent-initiated skill reads.

## Impact

Pi tool normalization and provider-neutral tool activity presentation. No filesystem reads, backend changes, or configuration changes.
