## Why

An assistant usage object with all counters zero replaces the last usable context sample, making the status percentage temporarily drop to zero. Missing usage already leaves the sample intact.

## What Changes

- Ignore all-zero usage samples in shared session accounting, retaining the last usable sample and cumulative counters.
- Cover empty samples before and after valid usage, same-request corrections, and genuinely lower nonzero usage.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None.

## Impact

`crates/e-tui/src/app.rs` only. Checked `openspec/specs/single-display-projection/spec.md` and the active `add-pi-agent-frontend/specs/pi-agent-frontend/spec.md` delta. Requirements remain unchanged: this restores reliable session usage presentation described in the client architecture without changing event surfaces or adapter contracts. Session replacement still clears usage; real nonzero reductions remain visible. Specs are skipped for this bug fix.
