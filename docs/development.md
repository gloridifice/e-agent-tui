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

`pie` accepts `--session <file>`, `--approve`, and `--no-approve`; use `pie --help` for the exact current interface. DSH client configuration is stored under `%APPDATA%\dshe\`; Pi frontend-only configuration is stored under `%APPDATA%\pie\`.

## Dependencies

Cargo uses the official crates.io registry. Add dependencies to the owning package manifest:

- DSH adapter or infrastructure: `crates/e-dsh/Cargo.toml`
- Pi adapter or infrastructure: `crates/e-pi/Cargo.toml`
- Frontend values or rendering: `crates/e-tui/Cargo.toml`

Commit the root workspace `Cargo.lock`. `crates/e-dsh/vendor/` and `tools/vendor-crates.mjs` are retired offline fallbacks; do not depend on them.

## Bridge development

Bridge source changes are not visible to a running DSH service until the deployed package is replaced and DSH is restarted. Follow [DSH integration](dsh-integration.md) for packaged and development deployment flows, and [testing](testing.md) for bridge and compatibility checks.

## Integration diagnostics

```powershell
node tools/probe-online.mjs       # check whether the bridge is online
node tools/hello-test.mjs         # send hello and print startup frames
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

Startup probes, snapshot capture, frame workloads, syntax workloads, Tracy, and timing environment variables are maintained in the [performance measurement methodology](subsystem/performance/methodology.md). Do not copy benchmark results into this guide; dated results belong under `docs/history/`.
