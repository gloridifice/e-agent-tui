<p align="center">
<img src="./readme/logo.png" width="128">
</p>

`e` is a terminal UI for [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness), designed to be concise, attention-friendly, lightweight, fast, and ready to use out of the box. It does not alter the behavior of the DeepSeek Harness core. The project is still in an early stage of development.

<p align="center">
<a href="#quick-start">Quick Start</a> | <a href="#build-yourself">Build Yourself</a> | <a href="#development">Development</a>
</p>

## Quick Start

> Windows and PowerShell are currently the primary supported environment.

Before installing, make sure [Git](https://git-scm.com/), [Node.js](https://nodejs.org/) with npm, and [Rust](https://rustup.rs/) with Cargo are available.

```powershell
# Install DeepSeek Harness.
npm install --global @deepseek-ai/dsh

# Clone e.
git clone https://github.com/gloridifice/e.git
cd e

# Install the bridge into e's dedicated DSH profile.
# If DSH_HOME is unset, $HOME\.dsh is used automatically.
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install

# Build and install dshe into Cargo's binary directory.
cargo install --path client --locked

# Start e in the directory you want to work in.
dshe
```

On first launch, `dshe` starts its dedicated DSH service automatically. Use `/login` to configure an API key or proxy, and `/model` to select a provider and model.

If PowerShell cannot find `dshe`, add `%USERPROFILE%\.cargo\bin` to `PATH`.

To update, pull the latest changes and run `cargo install --path client --locked` again. If `bridge/` changed, repeat the bridge installation commands and restart DSH.

### Build Yourself

To build without installing:

```powershell
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release
```

The executable is written to `target\release\dshe.exe`. The DSH bridge is still required; install it once with:

```powershell
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install
```

For a profiling build, enable the optional Tracy integration:

```powershell
cargo build --release --features tracy
```

## Development

This project is still a prototype. Issues are welcome, but pull requests are not being accepted yet.

Useful checks:

```powershell
cargo fmt --check
cargo clippy --all-targets
cargo test

cd bridge
npm test
cd ..

node tools/sync-protocol-contract.mjs --check
```

After changing `bridge/`, mount it again and restart DSH before testing it:

```powershell
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install
```

Development documentation is available in [`docs/`](docs/):

- [`docs/design.md`](docs/design.md) — architecture, design decisions, interaction rules, and implementation notes.
- [`docs/protocol.md`](docs/protocol.md) — generated WebSocket protocol reference. Its canonical source is [`bridge/protocol-contract.json`](bridge/protocol-contract.json).
- [`docs/tracy.md`](docs/tracy.md) — Tracy setup, startup timing, frame metrics, and performance benchmarks.
- [`docs/architecture-audit.md`](docs/architecture-audit.md) — current module dependency and architecture health audit.
