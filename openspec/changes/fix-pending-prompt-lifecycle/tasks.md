## 1. Local scheduling correction

- [x] 1.1 Process admitted terminal events before queued dispatch in both runners.
- [x] 1.2 Prevent immediate submission from overtaking existing local candidates.
- [x] 1.3 Add and run scoped regression checks for cancellation/dispatch ordering and FIFO admission.

## 2. Approved batch cancellation

- [x] 2.1 Replace selective cancellation with user-approved batch ASAP cancellation and verify official Pi/pinned DSH capabilities.
- [x] 2.2 Implement the shared local/in-flight/backend queue projection and cancellation barrier.
- [x] 2.3 Integrate Pi queue snapshots and correlated submit/clear acknowledgments without changing its official RPC launcher.
- [x] 2.4 Integrate DSH inbox observation, batch clear, serialized admission, and generated wire contract.
- [x] 2.5 Update help, README, and architecture for batch cancellation and pending lifecycle.
- [x] 2.6 Validate duplicate messages, admission/clear races, failures, session changes, ordering, and protocol compatibility; run formatting and Clippy for the cross-boundary Rust changes.

## Validation results

- Scoped Rust checks passed for the controller (28), queue model (8), queue rendering (3), Pi adapter (28 before adding the final deferred-model-failure regression; all 6 queue-specific tests passed afterward), DSH adapter (15), architecture (11), wire contract (2), and help (5).
- All 97 bridge tests passed. Generated-contract checks, strict OpenSpec validation, workspace formatting checks, and both binary compilation checks passed.
- Workspace all-target Clippy completed with zero errors and 72 remaining warnings outside the new queue implementation; no broad unrelated cleanup was performed.
- A real isolated Pi RPC smoke attempt was blocked before any RPC command could run: the installed Pi 0.85.0 package fails startup because @earendil-works/pi-server is missing. No model requests, global dependency changes, or live session mutations were made; temporary probe files were removed.
- DSH was not redeployed or restarted. Live deployment compatibility/GUI acceptance remains to be verified after rebuilding, running dshe setup, and restarting DSH. Wire protocol is now v10; old deployed clients/bridges must not be mixed.
