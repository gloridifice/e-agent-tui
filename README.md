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

1. Install pnpm: `npm install --global pnpm`
2. Install DeepSeek Harness: `npm install --global @deepseek-ai/dsh`
3. Install `e`: `cargo install dshe`
4. Run `dshe setup`

Then just run `dshe`, e will open dsh and e-tui.

On first launch, `dshe` starts its dedicated DSH service automatically. Use `/login` to configure an API key or proxy, `/model` to select a provider and model, and `/effort` to change the reasoning effort. Wide terminals show a themed Preview pane; narrow terminals keep the main conversation usable.

Key interactions:

- `Ctrl+Y`: enter Reading View; `j`/`k` select Blocks, `l` enters Item navigation, `y` copies the complete Block source, and `Esc` returns/exits.
- `Ctrl+P`: toggle full-screen Preview on narrow terminals.
- `Shift+Enter`: insert a newline; `Enter`: send; `Ctrl+Backspace`/`Ctrl+W` (or `Option+Backspace` on macOS): delete the word before the cursor.
- `Ctrl+H`: help; `Ctrl+N`: resume session; `PageUp`/`PageDown` or wheel: transcript scrolling; drag visible Transcript/Preview text to copy it.

If PowerShell cannot find `dshe`, add `%USERPROFILE%\.cargo\bin` to `PATH`.

If the project-managed DSH service or its lock gets stuck, run `dshe clean` to force-stop that service and remove `%DSH_HOME%\e.lock`. It does not stop a DSH service started outside `dshe`.

To update, pull the latest changes, run `cargo install --path crates/e-dsh --locked`, then run `dshe setup` to refresh the embedded bridge. Restart DSH if it is already running.

### Build Yourself

To build without installing:

```powershell
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release
```

The executable is written to `target\release\dshe.exe`. The DSH bridge is still required; install it once with:

```powershell
dshe setup
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

After changing `bridge/`, rebuild the client and re-run `dshe setup` so the embedded bridge is current, then restart DSH before testing it:

```powershell
cargo build --release
dshe setup
```

Development documentation is available in [`docs/`](docs/):

- [`docs/design.md`](docs/design.md) — architecture, design decisions, interaction rules, and implementation notes.
- [`docs/protocol.md`](docs/protocol.md) — generated WebSocket protocol reference. Its canonical source is [`bridge/protocol-contract.json`](bridge/protocol-contract.json).
- [`docs/tracy.md`](docs/tracy.md) — Tracy setup, startup timing, frame metrics, and performance benchmarks.
- [`docs/architecture-audit.md`](docs/architecture-audit.md) — current module dependency and architecture health audit.
