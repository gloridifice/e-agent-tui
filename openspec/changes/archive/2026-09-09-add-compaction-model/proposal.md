## Why
Allow cheaper compaction without changing the conversation model, and identify the model actually used in compaction feedback.

## What Changes
- Add `/compact set-model [<model_id>]` and `/compact unset-model`, reusing the model picker and canonical route resolution.
- Keep the override in the adapter runtime (DSH per session; Pi per process), without changing backend defaults or credentials.
- Route DSH manual and automatic summary calls through the override. Pi temporarily selects it only for manual compaction, restoring the original model and thinking level before dependent work.
- Normalize running and successful labels to lowercase with the actual model name when available.

## Capabilities

### New Capabilities
- `openspec/specs/compaction-model/spec.md`

### Modified Capabilities
- `openspec/specs/compaction-feedback/spec.md`

## Impact
Shared command/picker and timeline values; DSH compaction-purpose LLM waterfall and event projection; Pi correlated RPC ordering and restoration. Existing `pi-agent-frontend` native configuration and credential ownership remains unchanged. Failures, cancellation, queued requests, and history settlement need focused tests.
