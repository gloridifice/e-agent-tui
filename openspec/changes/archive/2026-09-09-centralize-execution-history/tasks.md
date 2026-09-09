## 1. Implementation

- [x] 1.1 Implement strict central root resolution and versioned workspace registry in both adapters, including normalized identity, atomic locked updates, and header-based recovery; verify isolation, concurrency, corruption/version/conflict failures with temporary-root tests.
- [x] 1.2 Route trace creation, resume, and queries exclusively through central storage; remove project ignore writes; preserve recording semantics and verify old files are untouched/unread with scoped execution-history tests for both adapters.
- [x] 1.3 Update README and client architecture for the new storage contract; run formatting, adapter Clippy and architecture checks, validate the spec delta, and inspect implementation changes before CLI archive.

Verification: 32 scoped history tests and 15 architecture tests passed. Workspace formatting and ordinary adapter Clippy passed; the extra `-D warnings` run was blocked by pre-existing `e-tui` warnings. No unrelated lint fixes were made. CLI archive performs the required main-spec merge after these completion checks.
