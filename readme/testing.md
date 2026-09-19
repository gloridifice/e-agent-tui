# Testing and validation

> Status: Current
> Authority: Repository validation policy and compatibility gates.

Choose checks according to the risk and scope of the change. Tests should protect behavior or architecture that can realistically regress; do not add them merely to increase coverage.

## Rust

Prefer the narrowest relevant test command, for example:

```powershell
cargo test --lib <module>
cargo test <test_name>
```

Do not run the full `cargo test --lib` or `cargo test` suite unless the user requests it or the change genuinely requires workspace-wide validation. Small changes may require no tests.

For small and medium Rust tasks, do not run workspace formatting or Clippy merely as an end-of-task ritual. For large or cross-cutting Rust changes, run:

```powershell
cargo fmt --all
cargo clippy --all-targets
```

Before committing Rust changes, `cargo fmt --all --check` must pass.

Tests must not depend on external configuration or theme files. Do not add tests for theme styling.

## Bridge

Bridge tests use `node:test` and live under `bridge/test/`:

```powershell
cd bridge
npm test
```

The package script uses `--experimental-test-isolation=none` to avoid `EPERM` when tests spawn sandboxed processes while remaining compatible with Node.js 22. File-layer tests must use a temporary home directory and must never touch the real `%DSH_HOME%`.

## Release assets and package archives

The bridge embedded in the published `e-dsh` crate is a generated mirror of the authoritative `bridge/` package. After any production bridge change, regenerate it from the repository root:

```powershell
node tools/sync-release-assets.mjs
```

Bridge tests run the corresponding `--check` mode. Before a release, also verify the actual package archives rather than relying only on workspace builds:

```powershell
node tools/verify-crate-packages.mjs
```

The verifier packages all three crates and checks the extracted adapters against the packaged `e-tui`, so it also works before a new synchronized `e-tui` version exists on crates.io.

## Protocol changes

The machine-readable authority is `bridge/protocol-contract.json`. After changing the protocol, run `node tools/sync-protocol-contract.mjs` to regenerate the [wire reference](../doco/specs/wire-protocol.md), fixtures, and package metadata, then verify from the repository root:

```powershell
node tools/sync-protocol-contract.mjs --check
```

Protocol changes require contract-driven coverage on both the Rust and Node.js sides.

## DSH compatibility gate

After a DSH upgrade or a compatibility-sensitive bridge change, first deploy the bridge to the profile under test by following [DSH integration](dsh-integration.md), run `dsh plugin --profile <p> install`, and restart DSH. Then run the gate against that deployed copy:

```powershell
$env:DSH_TUI_SMOKE_PROFILE = 'e'
cd bridge
npm run verify-dsh-upgrade
```

Set `DSH_TUI_SMOKE_PROFILE` to another deployed profile, such as `web`, when appropriate. The gate verifies public exports, helper compatibility, `/new`, cold resume, and `/model` routing; it must not be replaced by testing only the source checkout.
