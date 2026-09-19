# Development

> Status: Current
> Authority: Repository development workflow. User-facing installation and update instructions remain in the root [README](../README.md).

Windows and PowerShell are the primary development environment. Run commands from the repository root unless a command changes directory explicitly.

## Prerequisites

The DSH frontend requires Git, Rust/Cargo, Node.js/npm, pnpm, and DSH:

```powershell
npm install --global pnpm
npm install --global @deepseek-ai/dsh
```

The Pi frontend requires the official Pi runtime on `PATH`:

```powershell
npm install --global @earendil-works/pi-coding-agent
```

The Pi runtime must expose `queue_update`, `clear_queue`, and prompt admission acknowledgments (the 0.85.0 RPC contract). ASAP cancellation uses Pi's whole backend queue clear; see [key mappings](key-mapping.md) for the interaction semantics.

## Install from a checkout

```powershell
cargo install --path crates/e-dsh --locked
dshe setup

cargo install --path crates/e-pi --locked
pie
```

The repository also defines convenience aliases:

```powershell
cargo devdsh   # cargo install --path ./crates/e-dsh
cargo devpi    # cargo install --path ./crates/e-pi
```

See [DSH integration](dsh-integration.md) before changing or deploying the bridge.

## Build and run

The workspace default member is `e-dsh`; the executable artifacts are `dshe.exe` and `pie.exe`.

```powershell
cargo run
cargo run -p e-pi --bin pie
cargo build --release
cargo build --release --features tracy
```

`pie` accepts `--session <file>`, `--approve`, and `--no-approve`; use `pie --help` for the exact current interface. Both frontends share the [frontend configuration directory](../README.md#config); backend configuration and session state remain separate.

## Dependencies

Cargo uses the official crates.io registry. Add dependencies to the owning package manifest:

- DSH adapter or infrastructure: `crates/e-dsh/Cargo.toml`
- Pi adapter or infrastructure: `crates/e-pi/Cargo.toml`
- Frontend values or rendering: `crates/e-tui/Cargo.toml`

Commit the root workspace `Cargo.lock`. `crates/e-dsh/vendor/` and `tools/vendor-crates.mjs` are retired offline fallbacks; do not depend on them.

## Release

The three Rust packages inherit one workspace version and are released together with [`cargo-release`](https://github.com/crate-ci/cargo-release). Install the release tool once:

```powershell
cargo install cargo-release --locked
```

Before publishing, start from a clean `master` checkout and run the package verifier described in [testing](testing.md). To publish the version already declared in the workspace:

```powershell
cargo release --workspace             # dry run
cargo release --workspace --execute   # publish, commit, tag, and push
```

For later releases, select the increment explicitly; all package versions, the internal `e-tui` requirement, and `Cargo.lock` are updated together:

```powershell
cargo release patch --workspace
cargo release patch --workspace --execute
```

Cargo-release publishes `e-tui` before the two adapters and uses one shared `v<version>` tag. The pre-release hook clears cached local-registry sources for each selected package's current version and cleans that package's build artifacts when such sources exist. This prevents repeated dry runs from reusing an older same-version archive; downloaded crates.io sources are left intact, and normal publish verification stays enabled.

Never use `--allow-dirty` or `--no-verify` for an executed release. Uncommitted changes also make the dry run fail, even if all package builds pass. A crates.io version cannot be overwritten; yank a bad version and publish a new synchronized patch instead.

### GitHub binary releases

Pushing a stable `vX.Y.Z` tag triggers [the release workflow](../.github/workflows/release.yml). The tag must match every Rust workspace package version; prerelease tags are not accepted. The workflow checks generated bridge assets and protocol artifacts, runs bridge tests, then builds both `dshe` and `pie` for Windows x64 (MSVC) and Linux x64 (GNU, built on Ubuntu 22.04).

Each platform archive contains both executables, the README, and the license. After all builds succeed, the workflow uploads the archives and `SHA256SUMS` to a draft GitHub Release and publishes it with generated release notes. It uses the repository's `GITHUB_TOKEN` with release-job-only write permission; no additional publishing secret is needed. If publication fails after draft creation, inspect the draft and either finish publishing it or delete it before rerunning the publish job.

The existing cargo-release flow pushes the shared tag and therefore triggers binary publication automatically. To publish binaries without publishing to crates.io, commit synchronized package versions and `Cargo.lock`, then push the corresponding tag manually:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Use the actual workspace version and a new tag; do not move an existing release tag. The GitHub workflow does not publish crates to crates.io. Downloaded executables still require their external DSH/Pi runtimes and setup described in the root README.

## Bridge development

Bridge source changes are not visible to a running DSH service until the deployed package is replaced and DSH is restarted. Before a packaged build, run `node tools/sync-release-assets.mjs` so `e-dsh` embeds the current generated mirror. Follow [DSH integration](dsh-integration.md) for packaged and development deployment flows, and [testing](testing.md) for bridge and compatibility checks.

## Integration diagnostics

```powershell
node tools/probe-online.mjs       # check whether the bridge is online
node tools/hello-test.mjs         # send hello and print startup frames
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

Startup probes, snapshot capture, frame workloads, syntax workloads, Tracy, and timing environment variables are maintained in the [performance measurement methodology](performance.md). Do not copy benchmark results into this guide; dated results belong under `readme/history/`.
