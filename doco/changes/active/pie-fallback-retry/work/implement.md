# Implementation design

## Baseline and goals
`PiAdapter::record` maps native retry events but currently releases the frontend only on `agent_settled`. `main.rs` owns the deadline-driven selection loop. Native RPC has no continue/retry-last command; normal `prompt` is the supported continuation path. `e-tui` already projects retry activity rows. The working tree was clean before this change.

## Overall approach
Add adapter-local retry state and public configuration/deadline/tick entry points. Observe live assistant completions, never replay snapshots. Schedule only after native settlement, using monotonic deadlines and one-second display updates while waiting. Keep native streaming state distinct from pending fallback work, while reporting their union as frontend busy. Send a continuation prompt rather than replaying prior inputs or tools.

Add a provider-neutral `RetryProgress` timeline fact carrying correlation, activity state, and display message. It upserts a dedicated row whose terminal state is explicitly owned by the adapter rather than inferred from partial assistant output. Transient progress records have no surface sequence so countdown updates do not accumulate surface ownership. They outlive individual native turns and are excluded from turn-level forced settlement. Native `auto_retry_end` maps to `RetryFinished`, preserving its actual success, failure, or cancellation instead of displaying a new running attempt.

## APIs and data model
The adapter retains the consecutive failure count, latest assistant error, cancellation suppression, waiting deadline, correlated continuation admission, and current activity identity. Assistant success resets the budget. Native retries may run within each fallback attempt and do not consume additional fallback attempts. Interrupts suppress late error/settled records until a fresh run. New input, commands, and session replacement cancel waiting recovery. Continuation admission rejection is terminal, not automatically retried, preserving the existing admission contract. An accepted continuation is checked through correlated native state if no run start has arrived, so an extension-handled prompt cannot leave a phantom busy state. Late continuation/state replies are consumed without affecting replacement recovery.

The runner routes outputs outside state locks. A due retry waits for outstanding configuration, compaction, queue admission, and skill/fork operations. Countdown state is in memory only and is discarded on process exit or session replacement. Child transport failures remain fatal rather than replaying uncertain work.

## Algorithms and rules
Add `error_auto_retry` to the shared frontend schema and embedded defaults (true), with an on/off Behavior setting and English/Chinese UI strings describing Pi-only fallback recovery. The Pi runner synchronizes it each loop, including live settings changes and reload. Disabling cancels waiting recovery and suppresses further fallback attempts in the active run, without aborting an already running model call or changing native settings. DSH ignores the preference.

## Fixed decisions and discretion
Five additional attempts: 60, 300, 900, 1800, 1800 seconds. Recover any assistant `stopReason: error`, not user cancellation, isolated tool errors, unrelated extension errors, RPC controls, or rejected admissions. Native recovery always precedes fallback. No private Pi APIs, extension changes, dependencies, or new keybindings. Local naming and equivalent helper extraction are discretionary. No blocking design questions.

## Verification and documentation impact
Run scoped existing adapter, settings/config, projection, and architecture checks, plus compilation. Do not add or modify tests. Update current runtime, presentation, and configuration contracts; retain this package as active. Do not claim a real-provider 81-minute failure sequence was exercised unless actually run.
