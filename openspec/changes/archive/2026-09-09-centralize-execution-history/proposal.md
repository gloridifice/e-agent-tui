## Why

Execution traces should not pollute working directories. Central storage needs a discoverable, recoverable mapping from cache folders to backend-confirmed workspaces.

## What Changes

- Store traces exclusively under `<e-config>/cache/{e-dsh|e-pi}/history/<workspace-key>/<session-key>.jsonl`.
- Maintain a versioned `workspaces.json` in each history root, with stable path-derived workspace keys, atomic registration under a short-lived cross-process lock, and recovery from validated trace headers.
- Preserve session append/locking, bounded queries, output-free capture, and visible persistence failures. Never fall back to project-local storage.
- Do not read, migrate, delete, or otherwise modify legacy histories or ignore files. Native backend sessions remain untouched.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `session-execution-history`: centralized storage, workspace registry, and explicit new-root-only compatibility boundary.

## Impact

Both adapters' configuration-path resolution and execution-history stores; scoped persistence tests; README and client architecture. The frontend remains filesystem-free. Workspace identity uses versioned lexical normalization of an absolute confirmed cwd (without Git-root promotion, symlink resolution, or blanket case folding); SHA-256 keys isolate workspaces. Original cwd stays in trace headers alongside the normalized workspace identity. Moving a workspace creates a different bucket. Registry updates do not occur per event. Cache naming does not imply automatic eviction.
