## Context

The installed Rust executable currently cannot provision its required DSH bridge. Users must keep a repository checkout available, run `tools/mount-bridge.ps1`, and then separately run `dsh plugin --profile dshe install`. The normal launcher subsequently attempts to acquire DSH even when that preparation never happened, so users can receive token, timeout, or connection failures that describe symptoms instead of the missing setup step.

The bridge runtime is small (the package manifest, protocol contract, and JavaScript modules total less than 100 KiB), already has a compile-time relationship with `client/build.rs`, and must remain byte-aligned with the client wire contract. Windows is the primary environment, setup must preserve existing profile customization, and the resulting installed executable must not depend on repository files or PowerShell at execution time. DSH and its package installation facilities remain external prerequisites.

## Goals / Non-Goals

**Goals:**

- Make `dshe setup` the single user-facing command for provisioning or updating the dedicated `dshe` profile.
- Embed every bridge runtime file and a deterministic bundle digest in `dshe.exe` at build time.
- Make profile updates idempotent and preserve unrelated user-owned profile fields, dependencies, workspace settings, and patch entries.
- Record setup only after dependency installation and validation succeed.
- Reject every TUI startup path before launcher or terminal side effects when setup is missing, stale, or damaged.
- Emit concise English diagnostics that identify the failed condition and state the exact next action.

**Non-Goals:**

- Bundling Node.js, pnpm, DSH, bridge `node_modules`, or registry packages into `dshe.exe`.
- Automatically running setup during normal TUI startup.
- Removing `tools/mount-bridge.ps1` from development workflows for quickly synchronizing source changes or alternate profiles.
- Changing the WebSocket protocol or bridge runtime behavior.
- General-purpose management of arbitrary DSH profiles or plugins.

## Decisions

### 1. Generate an embedded bridge bundle at Rust build time

`client/build.rs` will enumerate a sorted allowlist consisting of `bridge/package.json`, `bridge/protocol-contract.json`, and regular `bridge/src/*.js` files. It will emit `rerun-if-changed` directives and generate an `OUT_DIR` Rust module whose entries use `include_bytes!`, so Cargo compilation places the bytes in the executable. Tests, tools, caches, and `node_modules` are excluded.

The build script will also compute a SHA-256 digest over each normalized relative path and its bytes. The path is part of the digest so file renames are observable. SHA-256 is used as a stable content identity, not as a trust boundary; the hashing crate is build-only and adds no runtime service dependency.

This is preferred over embedding an archive because the bundle is small, direct entries avoid extraction libraries and archive traversal concerns, and generated enumeration prevents a new runtime module from being silently omitted. It is preferred over embedding `node_modules` because installed dependencies are larger, platform-sensitive, and owned by DSH/pnpm compatibility resolution.

### 2. Route `setup` before all asynchronous TUI startup work

CLI parsing will produce a typed action such as `Setup` or `Run { url, resume_session_id }`. `Setup` will execute the installer and return without acquiring DSH, reading the token, opening WebSocket, or initializing the terminal. `Run` will call the setup readiness check before all of those side effects.

The reserved command is exactly `dshe setup`. `dshe install` will not be an alias; it will return an English diagnostic that says the command is not supported and instructs the user to run `dshe setup`. Existing URL/session positional behavior remains available after command parsing.

This explicit branch is preferred over setup-on-first-run because setup may perform network work, may need package-manager troubleshooting, and may require restarting an already running DSH process. These effects must not be hidden inside interactive startup.

### 3. Centralize DSH home and command resolution

Setup and launcher code will share resolution helpers. An unset, empty, or whitespace-only `DSH_HOME` resolves to the platform user home plus `.dsh`; the resolved value is explicitly passed to the DSH plugin child process so Rust and DSH cannot select different profile roots. A non-empty `DSH_HOME` is preserved.

The setup subprocess uses global `dsh` when available and otherwise the launcher's existing `npx -y @deepseek-ai/dsh` fallback. On Windows the fixed argument vector is invoked through the command shim handling already required by the launcher, with inherited stdout and stderr. Setup errors distinguish missing launch tools, process-spawn failure, and non-zero plugin exit, and each diagnostic provides a concrete remediation and retry instruction.

### 4. Install into a staged bridge directory and merge profile configuration conservatively

The setup module will write the embedded package to a temporary sibling directory under `%DSH_HOME%\profiles\dshe\packages`, then replace the installation-owned `dsh-tui-bridge` directory. Only normalized generated relative paths can be extracted. Individual profile metadata files are written through temporary files and atomic replacement where the platform permits.

For a fresh profile, setup creates the dedicated profile skeleton currently supplied by the mount script: the base and web-app bundles, the approved `dsh-win32` dependency/bundle, workspace configuration, and bridge patch entry. The approved `dsh-win32` version is held in one setup constant.

For an existing profile, setup parses and preserves its manifest, adds or corrects only `dependencies["dsh-tui-bridge"] = "workspace:*"`, ensures `packages/*` is in the workspace package list, and ensures the `tui-bridge` insert exists in `cordis.patch.yml`. It must handle both a top-level `[]` patch and an existing patch list without duplicating entries. Existing unrelated dependencies, bundles, pnpm settings, comments where practical, and patch entries remain untouched. Malformed files are not overwritten blindly; setup reports the exact path and asks the user to repair or restore it before retrying.

After files are prepared, setup runs `dsh plugin --profile dshe install`. A failed package-manager run leaves the prepared, rerunnable configuration in place but does not declare setup successful. Full rollback of pnpm state is intentionally not attempted because pnpm may already have changed its lockfile or module store; idempotent retry is safer than a partial synthetic rollback.

### 5. Use a successful-setup record plus structural validation

A successful setup is recorded atomically at `%DSH_HOME%\profiles\dshe\.dshe-setup.json` with a schema version, profile name, embedded bridge digest, and wire protocol version. The record is written only after the DSH plugin command succeeds and validation confirms the bridge source, profile dependency, workspace registration, patch registration, and installed workspace package/link exist.

Startup classifies readiness as:

- `Ready`: the record is valid, its digest matches this executable, and required installation structure exists.
- `Missing`: no successful record exists.
- `Outdated`: the record belongs to a different embedded bridge digest or unsupported record schema.
- `Damaged`: the record matches but one or more required profile/install artifacts are absent or no longer registered.

The record avoids hashing every bridge source file during every startup, while structural checks catch common deletion or profile-edit damage. A client-only update whose embedded bridge is unchanged remains ready; any bridge runtime change requires `dshe setup` again. Script-mounted legacy profiles intentionally classify as `Missing`, providing a one-time migration to the managed setup record.

### 6. Treat diagnostics as part of the user-facing contract

All errors originating in CLI setup routing, setup execution, readiness validation, and related launcher setup guidance will be English. They will identify what failed and include an exact next step. Readiness examples follow these forms:

- Missing: `DSH bridge setup is missing. Run 'dshe setup', then run 'dshe' again.`
- Outdated: `The installed DSH bridge does not match this dshe build. Run 'dshe setup' to update it, restart any running DSH service, then run 'dshe' again.`
- Damaged: `DSH bridge setup is incomplete at <path>. Run 'dshe setup' to repair it, restart any running DSH service, then run 'dshe' again.`

Path, parse, spawn, and package-manager failures will include the relevant path or command and a remediation appropriate to that failure. A bare internal error such as `connection refused`, `file not found`, or `exit code 1` is insufficient. Existing launcher messages that currently point to `mount-bridge.ps1` will instead point to `dshe setup` and, where needed, restarting DSH.

### 7. Keep source mounting as a development-only path

README installation and update instructions will place `cargo install --path client --locked` before `dshe setup`, because the installed executable now owns bridge provisioning. `AGENTS.md` and `docs/design.md` will describe the embedded setup contract and startup gate. The PowerShell script remains available for development synchronization, especially for the `web` profile, but it is no longer a user prerequisite or evidence that a current executable has completed setup.

## Risks / Trade-offs

- [A setup marker can become stale after manual profile edits] → Validate essential profile registrations and installed package paths on every startup; classify inconsistencies as damaged and provide the repair command.
- [Updating while DSH is already running does not hot-reload JavaScript] → Successful setup output and outdated/damaged startup diagnostics explicitly instruct the user to restart a running DSH service before launching again; protocol mismatch handling remains a final guard.
- [Profile text formats can contain user customization that is difficult to round-trip] → Use structured JSON mutation for `package.json`, narrow line-aware mutation for the workspace and patch files, preserve unrelated content, and fail rather than rewrite malformed input.
- [A plugin install can fail after files have been prepared] → Do not write the success record; retain an idempotent prepared state and print the package-manager command plus retry action.
- [The executable grows and bridge changes trigger Rust rebuilds] → The runtime bundle is small, excludes dependencies/tests, and already participates in the Rust build through the canonical protocol contract.
- [Legacy manually mounted installations stop launching once this client is installed] → Provide a single explicit migration, `dshe setup`, which reuses the existing profile without overwriting unrelated customization.

## Migration Plan

1. Build and release a client containing the embedded bundle, setup command, and readiness gate.
2. Update source-install documentation to install/reinstall the Rust executable first and then run `dshe setup`.
3. Existing users run `dshe setup` once; it merges the existing dedicated profile, installs dependencies, validates it, and writes the setup record.
4. Users restart any currently running DSH service and launch `dshe` normally.
5. On rollback, an older executable ignores the setup record. If its embedded/client protocol differs from the currently installed bridge, rerun the setup mechanism appropriate to that older release before launch.

## Open Questions

None. The command name, explicit startup gate, one-time migration behavior, embedded runtime scope, and English actionable-error policy are fixed by this change.
