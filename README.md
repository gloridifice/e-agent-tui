<p align="center">
<img src="./readme/logo.png" width="128">
</p>

`e` is a terminal UI for coding agents, designed to be concise, attention-friendly, lightweight, and fast. It provides `dshe` for [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness) and the experimental `pie` frontend for Pi. It does not alter either agent runtime. The project is still in an early stage of development.

<p align="center">
<a href="#quick-start-dsh">Start for DSH</a> | <a href="#quick-start-pi">Start for Pi</a> | <a href="#build-yourself">Build Yourself</a> | <a href="#development">Development</a>
</p>

## Quick Start DSH

> Windows and PowerShell are currently the primary supported environment.

Before installing, make sure [Git](https://git-scm.com/), [Node.js](https://nodejs.org/) with npm, and [Rust](https://rustup.rs/) with Cargo are available. And:

- Install pnpm: `npm install -g pnpm`
- Install DeepSeek Harness: `npm install -g @deepseek-ai/dsh`

Then install and setup `e` by:

```bash
cargo install dshe
dshe setup 
```

Run `e`:

```bash
dshe
```

On first launch, `dshe` starts its dedicated DSH service automatically. Use `/login` to configure an API key or proxy, `/model` to select a provider and model, and `/effort` to change the reasoning effort. Wide terminals show a themed Preview pane; drag the Bark-colored separator to resize it. Preview collapses below 16 columns, while narrow terminals keep the main conversation usable.

## Quick Start Pi

Install Pi and the `pie` executable from source, then run it in a project directory:

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install --path crates/e-pi --locked
pie
```

`pie` launches the official `pi --mode rpc` runtime and reuses Pi's native models, credentials, extensions, resources, and session files. Use `pie --session <session.jsonl>` to resume directly; `pie --approve` or `pie --no-approve` explicitly overrides Pi's native project-trust behavior. Run `pie --help` for all launch options.

## Build Yourself

To build without installing:

```bash
git clone https://github.com/gloridifice/e.git
cd e
cargo devdsh # alias of `cargo install --path crates/e-dsh`
cargo devpi # alias of `cargo install --path crates/e-pi`
```

```bash
dshe setup
dshe
# or for pi
pie
```

For a profiling build, enable the optional Tracy integration:

```bash
cargo build --release --features tracy
```


## Key interactions

- `Ctrl+Y`: enter Reading View; `j`/`k` select Blocks, `l` enters Item navigation, `y` copies the complete Block source, and `Esc` returns/exits.
- `Ctrl+P`: toggle full-screen Preview on narrow terminals.
- `Shift+Enter`: insert a newline; `Enter`: send; `Ctrl+Backspace`/`Ctrl+W` (or `Option+Backspace` on macOS): delete the word before the cursor.
- `Ctrl+H`: help; `Ctrl+N`: resume session; `PageUp`/`PageDown` or wheel: transcript scrolling; drag the pane separator to resize, or drag visible Transcript/Preview text to copy it.

If PowerShell cannot find `dshe`, add `%USERPROFILE%\.cargo\bin` to `PATH`.

If the project-managed DSH service or its lock gets stuck, run `dshe clean` to force-stop that service and remove `%DSH_HOME%\e.lock`. It does not stop a DSH service started outside `dshe`.

To update, pull the latest changes, run `cargo install --path crates/e-dsh --locked`, then run `dshe setup` to refresh the embedded bridge. Restart DSH if it is already running.

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

See the [`docs/` index](docs/README.md) for current client and bridge architecture, the generated WebSocket reference, performance methodology, and clearly separated historical material. The canonical wire source is [`bridge/protocol-contract.json`](bridge/protocol-contract.json).
