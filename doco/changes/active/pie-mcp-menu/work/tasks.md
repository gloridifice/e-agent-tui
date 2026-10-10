# Execution tasks

- [x] 1.1 Add the bounded public-API MCP companion and adapter control flow
  - Design: [implementation](implement.md)
  - Acceptance: Native command ownership remains intact; trusted, revision-checked configuration saves preserve unrelated fields; running saves emit a manual-reload reminder; idle saves reload safely; errors and stale responses do not claim success.
- [x] 1.2 Integrate the provider-neutral modal, native actions and semantic keys
  - Dependencies: 1.1
  - Acceptance: Responsive centered modal with full backdrop, server/tool inspection, confirmation, filter and masked callbacks; browsing `q` closes while editor `q` remains text; closing preserves drafts and cannot leak callbacks.
- [ ] 2.1 Synchronize delivered contracts and verify integration
  - Dependencies: 1.1, 1.2
  - Acceptance: Scoped existing tests, builds, workspace formatting/Clippy and isolated RPC/PTY checks pass; current documentation reflects delivered behavior; all deferrals are explicit.

## Verification
- Verification: Rust build/check and workspace Clippy passed (existing warnings). Scoped adapter tests: 28 passed; controller key-mapping tests: 18 passed; Windows VT tests: 27 passed. No tests were added or modified.
- Verification: Isolated native RPC inspected command ownership, preserved unrelated configuration/environment fields, rejected stale revisions, retained the deferred-reload flag, and executed native reload. Real PTY checks verified idle save/reload, F6, editor `q`, browsing `q`, preserved drafts, narrow detail navigation, and normal exit. Screenshots are disposable artifacts under the ignored prototype directory.
- Verification caveat: Existing reload tests pass 4/5. The empty-command-catalog assertion fails identically on an unchanged HEAD copy because fork/clone descriptors are present; no acceptance requirement or test was changed to hide it.
- Deferrals: A live model run during save and real MCP OAuth/network interactions were not exercised. The running-save policy is supported by the deferred flag probe and native immediate-command dispatch inspection, not claimed as a live-run end-to-end check. No second MCP client or independent connection probe was started.
- Blocked: Task 2.1 remains unchecked because its all-checks-pass acceptance is not satisfied by the existing baseline failure and the deferred live-run/OAuth verification. The change remains active; completion and archive were not requested.
