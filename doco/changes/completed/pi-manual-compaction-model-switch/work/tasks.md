# Execution tasks

- [x] 1.1 Represent the confirmed return route and independent compaction/control progress
  - Design: [baseline, approach, and state ownership](implement.md#1-baseline-and-goals).
  - Acceptance: The original snapshot, immutable compaction model, confirmed return tuple, session identity, compact result, and correlated user selection have distinct ownership. The no-selection path retains its existing restoration sequence. Native manual-start model capture is verified against the supported runtime; no assumption that all request options are frozen is introduced.
  - Verification: The adapter compiles with separate original/return routes, control handoff, compact completion, and selection correlation. Installed Pi 0.85.1 was inspected: manual `compaction_start` precedes synchronous evaluation of `_getSummarizationRequestAuth(this.model)`; thinking level remains a later native read.

- [x] 1.2 Implement bounded compaction-aware model-control admission
  - Dependencies: 1.1
  - Design: [admission and lifecycle rules](implement.md#4-algorithms-and-rules).
  - Acceptance: After the manual-start handoff, model reads use the cached catalog/confirmed route and model changes use the existing configuration barrier. Deferred model controls may pass held prompts, but not session/resource fences or earlier model controls. Draining runs on start and relevant responses. The total deferred capacity stays 64; all unrelated barriers and Interrupt behavior remain intact.
  - Verification: Disposable RPC simulation confirmed that a cached model read and selection pass an earlier held prompt after manual start. Source review confirmed the existing 64-entry overflow path, session/resource fences, common configuration/queue/skill barriers, and interrupt bypass remain in force.

- [x] 2.1 Confirm selections without leaking temporary or partial routes
  - Dependencies: 1.1, 1.2
  - Design: [confirmation and projection rules](implement.md#4-algorithms-and-rules).
  - Acceptance: Only the matching explicit selection's valid final state response updates the return model and effective effort. The header reflects confirmed changes before compaction finishes, while the compaction label stays fixed. Unrelated refreshes, partial failures, and stale-session responses cannot promote an unconfirmed route or settle the compaction command.
  - Verification: Disposable RPC simulation exercised two serialized successful selections and a rejected selection followed by a successful retry. Intermediate model responses stayed unprojected; only provider/model/effort/session-confirmed state promoted the return route. The captured compaction label remains independent in adapter state.

- [x] 2.2 Join in-flight selection before restoring and verifying the return route
  - Dependencies: 2.1
  - Design: [finalization and cancellation](implement.md#4-algorithms-and-rules).
  - Acceptance: Compact completion closes the control lane and waits for any admitted selection to settle. Restoration uses the latest confirmed tuple, or the original tuple if none superseded it. New selections wait through restoration, and prompts remain held until verification. Cancellation, compact errors, partial selection errors, disconnects, and restoration failures preserve the documented safety rules and visible outcomes.
  - Verification: Disposable RPC simulation delivered Compact completion while the second selection was in flight; no restore or prompt was emitted until confirmation, final restoration targeted the second route, and the held prompt was released only after verification. A rejected selection retained fallback state; compact failure restored the later confirmed retry. Existing cancellation and restoration-failure tests passed.

- [x] 3.1 Synchronize the delivered interaction contract
  - Dependencies: 2.2
  - Design: [current-document impact](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Update only the manual-compaction clause in the interaction spec to distinguish model controls from dependent work and original fallback from a newer confirmed choice. Preserve unrelated composer edits. Do not change architecture ownership, native APIs, configuration formats, automatic compaction, DSH, or shared frontend contracts outside this scope.
  - Verification: The [interaction contract](../../../../specs/interaction-and-sessions.md) now records the handoff, superseding confirmed route, and dependent-work barrier. No architecture, ADR, storage, protocol, README, bridge, key-help, or presentation contract was changed for this work.

- [x] 3.2 Run scoped validation and report unverified interleavings
  - Dependencies: 3.1
  - Design: [verification plan](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Run the listed existing Pi adapter tests, binary build, touched-file formatting/diff checks, and change-package check. Exercise the disposable-session scenarios when authorized and available, recording actual outcomes and limitations. Do not add or modify tests without separate user authorization. Do not report baseline test results as evidence for the new behavior, and do not complete or archive the change without a separate request.
  - Verification: See the command results below. No repository test was added or modified. The disposable simulator lived under the ignored build-output directory and was removed after use.

## Verification

Passed:

- `cargo test -p e-pi --lib adapter::compaction_tests::` — 7 passed.
- `cargo test -p e-pi --lib adapter::` — 51 passed.
- `cargo build -p e-pi --bin pie`.
- `cargo clippy -p e-pi --lib --no-deps -- -D warnings`.
- `rustfmt --edition 2021 --check` for the four touched Pi adapter files.
- `git diff --check` — only Git line-ending conversion notices.
- Disposable public-adapter RPC simulator — passed queued model read, prompt bypass, two confirmed selections, Compact/selection completion race, rejected selection recovery, compact failure, latest-route restoration, verification, and delayed prompt release.
- `doco check pi-manual-compaction-model-switch` — rerun after task synchronization.

Limitations:

- A live paid-provider compaction was not run because this execution did not include authorization to incur provider cost or use credentials. The simulator drove the real adapter API and typed RPC records but not a live Pi child/provider.
- Combined “confirmed selection then user cancellation” was not separately simulated. Existing cancellation tests and the same return-route restoration path passed, but that exact combined interleaving remains manual-validation coverage.
- Full dependency Clippy with `-D warnings` was attempted but is not a clean repository gate: it stopped on 31 pre-existing `e-tui` warnings outside this change. The scoped `e-pi --no-deps` Clippy check passed.

All implementation tasks are delivered, but the change remains active. It was not completed or archived.
