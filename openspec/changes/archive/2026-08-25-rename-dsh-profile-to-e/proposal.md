## Why

The product is named **e**, but its dedicated DSH profile is currently named `dshe`. Renaming the profile to `e` makes the DSH composition identity consistent with the product while preserving the `dshe` executable and its user configuration paths.

## What Changes

- **BREAKING**: Change the dedicated DSH profile selected by `dshe` from `dshe` to `e`.
- Provision, validate, diagnose, and development-mount the bridge under `%DSH_HOME%\profiles\e`.
- Migrate an existing project-owned `dshe` profile to `e` during setup when no `e` profile exists, preserving its custom dependencies and patch configuration.
- Refuse to overwrite a pre-existing `e` profile through automatic migration.
- Update user/developer guidance and compatibility-smoke examples to name the `e` profile.

## Capabilities

### New Capabilities
- `dedicated-e-profile`: Provision, migrate, validate, and launch the dedicated `e` DSH profile.

### Modified Capabilities
- None.

## Impact

- `crates/e-dsh/src/dsh_env.rs`, `setup.rs`, launcher diagnostics, bridge diagnostics, and focused tests.
- `tools/mount-bridge.ps1` and profile-targeted smoke examples.
- `AGENTS.md`, `docs/client.md`, and `docs/design.md`.
- Existing users must run an updated `dshe setup` and restart their DSH service; the `dshe` binary, `.dshe-setup.json`, `%APPDATA%\dshe`, and `%DSH_HOME%\e.lock` remain unchanged.
