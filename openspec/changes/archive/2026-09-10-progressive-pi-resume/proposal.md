## Why

Opening Pi Resume currently synchronously parses whole session files and hides oversized conversations. Keep the picker responsive and valid large sessions selectable.

## What Changes

- Enumerate native session candidates asynchronously, newest modification first, without a fixed file-count cutoff.
- Load metadata in batches of twice the rendered list height; append completed batches and request more near the loaded boundary.
- Search progressively scans unloaded candidates, preserving selection and rejecting obsolete page/workspace results.
- Read names backwards within a byte budget and use bounded first-user/default fallbacks instead of hiding valid oversized sessions.
- Replace Resume row paths with left-aligned Mist-equivalent titles and right-aligned Bark-equivalent last-modified dates. Pi supplies local file modification time; unavailable backend dates stay absent.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pi-agent-frontend`: progressive native Resume roster and resilient bounded metadata discovery.
- `input-page`: Resume title/date row presentation.

## Impact

`e-pi` owns background filesystem enumeration, indexing, and worker lifecycle. `e-tui` owns neutral paging demand, geometry, and result admission. Pi remains the sole session writer and switch authority. DSH's existing list protocol and backend remain unchanged. Risks are stale asynchronous results, selection movement, incomplete search indication, and malformed or unusually large JSONL records.
