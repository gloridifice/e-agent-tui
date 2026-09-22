<!-- doco:lifecycle v=1 created-at=2026-09-22T02:10:34Z completed-at=- archived-at=- -->
# Pi fallback retry

## Purpose
Pi's native retry policy leaves some model errors unrecovered and provides insufficient feedback during long waits. Add a bounded recovery layer without disabling or racing native recovery.

## Scope and acceptance
- After an accepted model run settles with an assistant error of any category, retry up to five times with delays of 1, 5, 15, 30, and 30 minutes. Native retries and automatic compaction recovery run first and remain unchanged.
- Update one message-area activity with attempt count and a seconds-resolution countdown; show running, success, cancellation, and exhaustion explicitly.
- Use a normal RPC continuation prompt in the existing conversation. Do not replay the original task, tools, slash commands, or rejected prompt admissions.
- Cancel pending recovery on interruption, replacement session, or a superseding user prompt/command. Success resets the failure budget; cancellation must not restart recovery from a late settled event.
- Keep policy and deadlines in e-pi and presentation provider-neutral. Waiting remains busy to the frontend, so after-turn queues and temporary model restoration cannot race recovery.
- Add a default-on `error_auto_retry` Behavior setting. Changes apply live and persist through the existing frontend config. Disabling cancels pending fallback recovery without changing native retry settings; DSH ignores this preference.
- Non-goals: restarting a crashed child, retrying individual tool/extension/control errors, changing native retry settings, new keybindings, and adding or modifying tests.
- Update current runtime, presentation, and configuration contracts for the delivered behavior.

## Result
Pending — not completed.
