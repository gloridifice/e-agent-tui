## Context

`dshe` currently declares `dshe` as its dedicated DSH profile. The profile name is consumed by Rust command construction, setup paths and setup records, repair diagnostics, the PowerShell development mount default, smoke commands, and documentation. Existing users may have custom dependencies and Cordis patches under `%DSH_HOME%\profiles\dshe`.

## Goals / Non-Goals

**Goals:**

- Make `e` the sole profile launched and provisioned by current `dshe` builds.
- Preserve an existing dedicated `dshe` profile, including arbitrary valid profile content, by moving it to `e` on the first updated setup when `e` is absent.
- Keep the executable name, bridge package name, bridge route, global token, lock name, configuration directory, and setup-record filename stable.
- Give all operator and developer guidance the correct `e` profile commands.

**Non-Goals:**

- Merging two independently customized `dshe` and `e` profiles.
- Removing the legacy `dshe` directory when an `e` directory already exists.
- Changing DSH global configuration, session persistence, WebSocket protocol, or the public `dshe` CLI.

## Decisions

### One source of truth for the runtime profile

`dsh_env::PROFILE_NAME` becomes `"e"`; path resolution, command construction, setup-record validation, and new manifest naming continue to derive from it. User-facing diagnostic commands are formatted from that constant where practical.

This avoids divergent setup and launcher targets. Duplicating the new literal across diagnostics would be simpler short-term but risks another incomplete rename.

### Migrate by directory rename during setup

Before provisioning `%DSH_HOME%\profiles\e`, setup checks whether `e` is absent and legacy `dshe` exists. In that case it renames the legacy directory to `e`, then performs normal idempotent provisioning and plugin installation.

A directory rename preserves all existing dependencies, lock files, packages, patches, and user configuration without attempting to interpret or merge them. Copying selected files would be lossy and could leave stale state. The existing `.dshe-setup.json` record is retained by the move but is rewritten after a successful setup with `profile: "e"`.

If `e` already exists, setup never moves, merges, or deletes `dshe`; it provisions `e` as the explicitly selected current target and leaves the legacy directory untouched.

### Retain `dshe`-named product state

The profile name is a DSH composition identifier, not a product rename. The executable stays `dshe`, configuration remains under `%APPDATA%\dshe`, the setup record remains `.dshe-setup.json`, and the managed-service lock remains `e.lock`.

## Risks / Trade-offs

- [A running legacy DSH service continues serving the old profile after migration] → Documentation instructs users to run `dshe clean` or restart DSH after setup; setup does not kill a user-managed process.
- [An `e` profile already exists] → Setup leaves `dshe` untouched rather than guessing how two profile trees should merge.
- [Directory rename fails due to locks or permissions] → Setup returns an actionable error and makes no partial copy; the user can stop DSH or repair filesystem access and retry.

## Migration Plan

1. Install the updated `dshe` binary.
2. Run `dshe clean` when the project-managed old service is active, or stop a manually started DSH service.
3. Run `dshe setup`; it renames `profiles\dshe` to `profiles\e` only when `e` is absent, installs bridge dependencies, and writes the updated setup record.
4. Verify with `dsh --profile e --dump-config`, then run `dshe`.
5. To roll back before later modifications, rename `profiles\e` back to `profiles\dshe` and use the previous binary. No session or global configuration migration is required.

## Open Questions

None.
