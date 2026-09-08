## Why

The Pi adapter currently classifies the built-in `edit` tool correctly but routes its arguments through the generic JSON Preview fallback and discards Pi's event-authored result patch. Consequently `pie` shows edit JSON instead of the mutation diff already available on the RPC stream.

## What Changes

- Normalize Pi `edit` call arguments into provider-neutral mutation hunks instead of generic JSON.
- Preserve Pi's authoritative result-time unified patch and use it to settle the same tool Preview target.
- Keep live RPC, session replay, and result-before-call history ordering equivalent.
- Recognize path-bearing mutation hunks as file activities so existing read/edit folding remains available.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `structured-tool-preview`: Pi edit calls and results must use their event-authored mutation data and converge on an authoritative settled diff.

## Impact

Affected code is limited to the Pi RPC adapter and kernel-neutral tool projection/Preview facts in `crates/e-pi` and `crates/e-tui`. No Pi runtime changes, filesystem reads, client-computed diffs, wire protocol changes, or new dependencies are required.
