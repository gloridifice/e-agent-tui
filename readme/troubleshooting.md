# Troubleshooting

> Status: Current
> Authority: Operational diagnosis entry points. Architecture and exact behavior remain governed by the linked current documents, source, and tests.

## Bridge changes do not appear

A running DSH service does not reload a replaced bridge automatically. Use either the packaged `cargo install` + `dshe setup` flow or the development mount flow, then restart DSH. See [DSH integration](dsh-integration.md).

If the route is still unavailable, inspect the selected profile and reinstall its plugin:

```powershell
dsh --profile e --dump-config | Select-String tui-bridge
dsh plugin --profile e install
```

Use the same profile name throughout the mount, install, inspection, and smoke-test commands.

## `dshe` refuses to start

`dshe` validates `%DSH_HOME%\profiles\e\.dshe-setup.json` and the installed profile structure before launcher or terminal startup. Missing, stale, malformed, or damaged setup produces guidance to run:

```powershell
dshe setup
```

Restart DSH after repair. See [setup readiness](dsh-integration.md#setup-readiness) for the ownership model.

## Managed DSH service or lock is stuck

```powershell
dshe clean
```

This force-stops only the project-managed DSH process recorded by the launcher and removes stale `%DSH_HOME%\e.lock` state. It does not stop a DSH service started outside `dshe`.

## Startup is unexpectedly slow

Enable per-stage startup timing:

```powershell
$env:DSH_TUI_TIMING='1'
dshe
```

Use `node tools/probe-startup.mjs` to isolate bridge attach and snapshot production. For snapshot capture, frame timing, Tracy, and interpretation guidance, follow the [performance measurement methodology](performance.md). Historical measurements are evidence only and must not be treated as current targets.
