## Why

Installing the dedicated DSH bridge currently depends on a repository checkout, a PowerShell mount script, and a separate `dsh plugin` command. Packaging the bridge inside `dshe` makes setup repeatable from the installed executable, while an explicit startup preflight prevents users from reaching less actionable launcher, token, or connection failures when setup has not completed.

## What Changes

- Add a `dshe setup` command that writes the bridge embedded in the Rust executable into the dedicated `dshe` DSH profile, configures the profile idempotently, and runs the profile plugin installation.
- Record successful setup against the embedded bridge bundle so a later client can distinguish ready, missing, damaged, and out-of-date setup states.
- **BREAKING**: Require a successful, current `dshe setup` before any TUI startup path; an existing script-mounted profile without the setup record requires a one-time `dshe setup` migration.
- Refuse startup before spawning DSH, reading the token, connecting WebSocket, or initializing the terminal when setup is not ready.
- Make all setup and preflight errors English, explicit about the failed condition, and actionable by naming the exact next command or remediation step.
- Replace the source-install documentation's `mount-bridge.ps1` plus `dsh plugin --profile dshe install` flow with `dshe setup`; retain the mount script only as a development synchronization tool.

## Capabilities

### New Capabilities
- `self-contained-dsh-setup`: Embedded bridge packaging, idempotent `dshe setup`, setup readiness tracking, startup gating, and actionable English diagnostics.

### Modified Capabilities

None.

## Impact

- Rust build generation and package metadata under `client/build.rs` and `client/Cargo.toml`.
- CLI routing and startup order in `client/src/main.rs`.
- A new client setup module plus shared DSH command/home resolution in `client/src/lib.rs` and `client/src/launcher.rs`.
- The dedicated profile under `%DSH_HOME%\profiles\dshe`, including an installation-owned setup record.
- Launcher diagnostics and source-install documentation in `README.md`, `AGENTS.md`, and `docs/design.md`.
- No wire-protocol payload changes; the existing bridge contract remains canonical.
