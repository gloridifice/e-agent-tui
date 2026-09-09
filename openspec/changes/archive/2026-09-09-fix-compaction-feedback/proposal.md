## Why
Issue #23 reports that completed compaction still looks active and leaves a stale context percentage visible.

## What Changes
- Replace the successful compaction label with completion feedback.
- Invalidate context percentage after successful compaction until fresh nonzero assistant usage arrives, preserving session totals and history paging semantics.

## Capabilities
### New Capabilities
- `compaction-feedback`
### Modified Capabilities
None.

## Impact
Shared activity projection, session usage state, status rendering, and focused regression tests. Failed compaction must not claim success or invalidate usage.
