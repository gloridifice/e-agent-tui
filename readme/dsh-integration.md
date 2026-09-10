# DSH integration

> Status: Current
> Authority: DSH profile setup, bridge deployment, and upgrade workflow. Exact setup state and launcher behavior remain governed by active specifications, source, and tests.

`dshe.exe` embeds the production bridge package and canonical protocol contract at build time. The bridge runs inside a dedicated DSH profile named `e`; replacing files in the repository does not update that deployed copy or a running DSH service.

## DSH home and profiles

A non-empty `DSH_HOME` selects the DSH home. When it is unset, empty, or whitespace-only, setup and launcher behavior fall back to the platform user home joined with `.dsh`.

The relevant profiles are:

- `e` — the profile provisioned and launched by `dshe`.
- `web` — an optional development profile supported by the mount script.
- `dshe` — the legacy dedicated profile name.

On setup, if `profiles\e` is absent and `profiles\dshe` exists, the legacy directory is moved to `profiles\e` before normal provisioning. Stop DSH around this migration. If `e` already exists, setup does not merge or remove the legacy tree.

## Packaged setup and updates

The normal source-install and update flow is:

```powershell
node tools/sync-release-assets.mjs   # required after changing bridge/
cargo install --path crates/e-dsh --locked
dshe setup
```

`dshe setup` checks that pnpm is executable before modifying the profile, materializes the embedded bridge, registers it in the profile, runs the DSH plugin installer, validates the installation, and only then records successful setup. Restart any running DSH service after setup so it loads the new bridge.

Use this packaged flow when validating the bridge embedded in the executable. It does not depend on the source checkout after installation.

## Development mount

For faster bridge iteration without rebuilding `dshe`, mirror the checkout into a selected profile:

```powershell
.\tools\mount-bridge.ps1 -Profile e    # or: -Profile web
dsh plugin --profile e install          # use the same selected profile
```

Restart the selected DSH service after the copy and plugin installation. The script creates the profile skeleton when needed and mirrors the physical bridge package; a junction is not suitable because package resolution must occur under the profile's module tree.

Keep `tools/mount-bridge.ps1` compatible with Windows PowerShell 5.1. PowerShell variable names are case-insensitive, an empty `DSH_HOME` must retain the user-home fallback, and Node JSON files must be written as UTF-8 without a BOM.

## Setup readiness

Successful setup is recorded at `%DSH_HOME%\profiles\e\.dshe-setup.json`. The record identifies the setup schema, profile, embedded bridge digest, and wire protocol. Normal `dshe` startup checks that the record matches the executable and that required profile structure still exists before starting DSH or entering the terminal.

A missing, outdated, malformed, or structurally damaged setup is repaired by rerunning:

```powershell
dshe setup
```

Then restart DSH. Do not hand-edit the setup record.

## Upgrade verification

After changing DSH compatibility metadata or making a compatibility-sensitive bridge change, deploy and restart first, then run the deployed-copy gate described in [testing](testing.md). Testing only `bridge/src` does not prove that the selected profile contains the current package.
