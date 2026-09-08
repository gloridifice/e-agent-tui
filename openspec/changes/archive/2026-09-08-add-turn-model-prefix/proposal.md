## Why

Users need to route one conversation turn to a marked model without changing the model used by subsequent turns.

## What Changes

- Resolve a leading `//<mark>` through saved model marks, remove it before sending, and preview the model name in Umber without editing the draft.
- Keep the temporary route for steering and tool continuations; restore the original selection before after-turn dispatch.
- Italicize the temporary status-bar model and gate dependent prompts on confirmed selection/restoration.

## Capabilities

### New Capabilities

- `turn-model-prefix`

### Modified Capabilities

None.

## Impact

Shared frontend composer, queue/controller, model lifecycle, help, and status rendering. Reuses provider-neutral model selection on both adapters. Checked `input-page`, `deferred-new-conversation`, `host-model-selection-compatibility`, and the active `fix-pending-prompt-lifecycle` delta; their existing selection, materialization, and queue contracts remain applicable.
