## 1. Embedded Bridge Bundle

- [x] 1.1 Extend `client/build.rs` to enumerate the production bridge allowlist deterministically, emit Cargo rebuild tracking, generate `include_bytes!` bundle entries, and compute a path-and-content SHA-256 bundle digest.
- [x] 1.2 Add the build-only hashing dependency, update the workspace lockfile, and expose the generated embedded files/digest through a new client setup module without embedding tests, tools, caches, or `node_modules`.
- [x] 1.3 Add build/setup tests that verify all current `bridge/src/*.js` runtime modules plus the package manifest and protocol contract are represented exactly once and that changed paths/content affect bundle identity.

## 2. Profile Provisioning

- [x] 2.1 Implement shared DSH home resolution so unset, empty, and whitespace-only values use the default `.dsh` home and custom values are preserved consistently by launcher and setup.
- [x] 2.2 Implement staged extraction and replacement of the installation-owned `packages/dsh-tui-bridge` tree using only normalized generated paths and actionable English filesystem errors.
- [x] 2.3 Implement fresh dedicated-profile creation with the approved base/web/`dsh-win32` skeleton and bridge registration.
- [x] 2.4 Implement conservative, idempotent mutation of existing `package.json`, `pnpm-workspace.yaml`, and `cordis.patch.yml`, including the top-level `[]` patch case, while preserving unrelated valid configuration and refusing malformed input with actionable English errors.
- [x] 2.5 Add focused temporary-directory tests for fresh setup, customized-profile preservation, bridge replacement, malformed files, and repeated setup without duplicate registrations.

## 3. Plugin Installation and Setup Record

- [x] 3.1 Refactor launcher command resolution into reusable boot/plugin argument construction and process execution seams, including global `dsh`, `npx` fallback, Windows shim handling, inherited output, and explicit resolved `DSH_HOME` propagation.
- [x] 3.2 Implement `dsh plugin --profile dshe install` execution with distinct actionable English diagnostics for missing prerequisites, spawn failure, and non-zero package-manager exit.
- [x] 3.3 Implement post-install validation for bridge files, profile dependency, workspace registration, patch registration, and the installed workspace package/link.
- [x] 3.4 Implement atomic `.dshe-setup.json` persistence only after validated success, carrying the record schema, profile, embedded bridge digest, and wire protocol version.
- [x] 3.5 Add scripted-process and temporary-profile tests proving failed commands or validation never record the attempted bundle as ready and successful/repeated setup records current state.

## 4. CLI Routing and Startup Gate

- [x] 4.1 Add typed CLI routing for `Setup` and existing TUI run arguments; make `dshe setup` return without launcher, token, WebSocket, or terminal work, and make `dshe install` fail with an English instruction to run `dshe setup`.
- [x] 4.2 Implement readiness classification for ready, missing, outdated, and damaged setup using the record plus structural checks.
- [x] 4.3 Gate every TUI startup path on readiness before DSH acquisition and all terminal/network side effects, with condition-specific English messages that include the exact setup, restart, and retry actions.
- [x] 4.4 Replace launcher guidance that references `mount-bridge.ps1` with actionable `dshe setup` guidance and preserve relevant startup failure context.
- [x] 4.5 Add regression tests proving missing, outdated, damaged, and legacy marker-less profiles cannot reach launcher side effects, while ready profiles preserve normal startup behavior and all setup-related expected messages are English and actionable.

## 5. Documentation and Verification

- [x] 5.1 Update `README.md` so source installation and updates install the Rust executable before running `dshe setup`, and describe restarting DSH after bridge updates without expanding implementation detail.
- [x] 5.2 Update `AGENTS.md` and `docs/design.md` in English with the embedded setup ownership, marker/readiness contract, startup gate, development-only role of the mount script, and actionable-error policy.
- [x] 5.3 Run focused setup/launcher tests, `cargo fmt --all --check`, `cargo clippy --all-targets`, and `node tools/sync-protocol-contract.mjs --check`.
- [x] 5.4 Manually verify `cargo install --path client --locked`, `dshe setup`, config visibility through `dsh --profile dshe --dump-config`, idempotent rerun, startup refusal before setup, and setup from an installed executable after the source checkout is unavailable.
