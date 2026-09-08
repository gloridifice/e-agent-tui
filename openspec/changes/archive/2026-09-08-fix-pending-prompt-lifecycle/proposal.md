## Why

ASAP prompts leave the frontend queue before the agent consumes them, so their pending indicators and newest-first cancellation disappear too early. Both runners also dispatch a queued prompt before handling an already-received Escape key.

## What Changes

- Handle an admitted terminal event before claiming a queued prompt in both runners.
- Preserve submission order independently of ASAP-first display and dispatch order.
- Retain candidates until authoritative consumption, including backend-queued steering prompts.
- Cancel all pending ASAP messages as one acknowledged, non-interrupting batch. Preserve local after-turn messages; cancel those newest-first only when no ASAP candidates remain.
- Use official Pi queue_update/clear_queue and DSH inbox snapshots/splice. Pi batch clear also affects extension-origin backend queues, as explicitly accepted by the user.

## Capabilities

### New Capabilities

- `pending-prompt-lifecycle`: Ordered, visible, cancellable pending prompts across frontend and backend admission.

### Modified Capabilities

None.

## Impact

Shared e-tui controller/state, the dshe and pie runner ordering, provider adapters, and potentially the DSH wire contract and upstream Pi RPC capabilities. Existing configurable key mappings must be preserved. No runtime dependency files outside the repository will be patched.
