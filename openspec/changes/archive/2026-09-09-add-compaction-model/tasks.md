## Implementation
- [x] Add shared compaction subcommands and picker targeting; test direct selection and picker isolation.
- [x] Add DSH per-session override routing and actual-model feedback; test summary-only routing and clearing.
- [x] Add Pi manual selection/compaction/restoration ordering; test success, failure, cancellation, and automatic behavior.
- [x] Normalize compaction timeline feedback and test settlement/history behavior.
- [x] Run scoped tests, formatting and Clippy; update public workflow and architecture documentation; synchronize assets. Report deployment verification separately if unavailable.

Validation passed: Pi adapter tests (40), frontend input tests (141), command tests (7), focused compaction tests (Pi 5 / frontend 6 / DSH 1), Rust architecture gate (16), bridge suite (108), protocol/assets checks, workspace formatting, and workspace all-target Clippy (existing warnings remain). Live deployed-profile verification is deferred: no running DSH service was replaced/restarted and no paid model request was made.
