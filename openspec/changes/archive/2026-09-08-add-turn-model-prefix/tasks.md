## 1. Implementation

- [x] 1.1 Add prefix parsing and presentation-only inline preview, preserving input/history/image boundaries; run focused composer tests.
- [x] 1.2 Implement confirmed temporary selection and restoration across immediate, steering, after-turn, failure, and deferred-new paths; run scoped controller and adapter checks.
- [x] 1.3 Add italic temporary status and document the public syntax in help and README; run rendering regressions and cross-cutting Rust checks.

Verification: focused composer, controller, queue, status, main-pane, localization, Pi adapter, DSH adapter, and architecture tests passed. Workspace formatting passes. Workspace all-target Clippy completes with existing warnings; the optional `-D warnings` run fails on existing enum-size and other unrelated lints. No live provider call or installed-binary deployment was performed. Specification synchronization and archive use the repository CLI completion workflow after these implementation tasks.
