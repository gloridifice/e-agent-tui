<p align="center">
<img src="./readme/logo.png" width="128">
</p>

`e` is a terminal UI for coding agents, designed to be concise, attention-friendly, lightweight, and fast. It provides `dshe` for [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness) and the `pie` frontend for Pi.
It does not alter either agent runtime. The project is still in an early stage of development.

<p align="center">
<a href="#quick-start-dsh">Start for DSH</a> | <a href="#quick-start-pi">Start for Pi</a> | <a href="#build-yourself">Build Yourself</a> | <a href="#development">Development</a>
</p>

https://github.com/user-attachments/assets/45a75c55-7e3b-4af3-b1e2-1c6306b80358

## Quick Start DSH

> Windows and PowerShell are currently the primary supported environment.

Before installing, make sure [Git](https://git-scm.com/), [Node.js](https://nodejs.org/) with npm, and [Rust](https://rustup.rs/) with Cargo are available. And:

- Install pnpm: `npm install -g pnpm`
- Install DeepSeek Harness: `npm install -g @deepseek-ai/dsh`

Then install and setup `e` by:

```bash
cargo install e-dsh
dshe setup
```

Run `e`:

```bash
dshe
```

On first launch, `dshe` starts its dedicated DSH service automatically. Use `/login` to configure an API key or proxy, `/model [provider/model]` to select a model, and `/effort [level]` to change the reasoning effort. Wide terminals show a themed Preview pane; drag the Bark-colored separator to resize it. Preview collapses below 16 columns, while narrow terminals keep the main conversation usable. The interface defaults to English; Simplified Chinese is available with `language = "zh-CN"` in the config (English remains the fallback).

## Quick Start Pi

Install Pi and the `pie` executable from source, then run it in a project directory:

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi
pie
```

`pie` launches the official `pi --mode rpc` runtime and reuses Pi's native models, credentials, extensions, resources, and session files. Inside `pie`, use `/login [provider]` for Pi's native API-key, OAuth, browser, device-code, and multi-step setup flows, and `/logout` to remove a stored credential. Use `pie --session <session.jsonl>` to resume directly; `pie --approve` or `pie --no-approve` explicitly overrides Pi's native project-trust behavior. Run `pie --help` for all launch options.

Type `@` in the composer to browse project-relative paths. Use ↑/↓ to select, Tab or Enter to fill (directories continue browsing), and Esc to dismiss; Enter sends only after file completion closes. Paths containing spaces are quoted automatically.

## Prebuilt binaries

Download the Windows x64 ZIP or Linux x64 tarball from [GitHub Releases](https://github.com/gloridifice/e/releases). Each archive includes `dshe` and `pie`; extract them into a directory on `PATH`. `SHA256SUMS` provides archive checksums. Rust/Cargo is not needed for these downloads, but the runtime prerequisites above still apply; run `dshe setup` before using DSH.

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

## Features

- **Model default effort:** Use `/model <model-id> set-default-effort <effort>` to save a default for future model selections without changing the current session's effort.
- **Execution history:** Use `/history` to view slow operations and the usage timeline, or `/history copy` to copy execution metadata that may contain sensitive command arguments.
- **Compaction model:** Use `/compact set-model` or `/compact unset-model` to configure a shared override for DSH compaction and Pi manual compaction.

## Config

`e`'s config is under:

- Windows: `%APPDATA%\e`
- Linux: `$XDG_CONFIG_HOME/e` (default `~/.config/e`)
- macOS: `~/.config/e`

You can run `/econfig` in `e` to open or show the config path of your device.

- `config.toml`: Settings configuration. `/econfig` prints its path in the message pane.
- `key_mapping.toml`: Key mapping overrides.
- `themes/`: custom theme folder.

- **Key mapping:** Customize `key_mapping.toml` using the [key mapping guide](readme/key-mapping.md) and [default bindings](crates/e-tui/assets/default_key_mapping.toml).
- **Themes:** Use `/theme` to switch themes or add custom TOML files under `themes/`, following the [bundled examples](crates/e-tui/assets/themes/).

Use `/reload` to reload configuration, themes, and backend resources; replacing DSH plugin code still requires a service restart.

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
node tools/sync-release-assets.mjs --check
```

After changing `bridge/`, regenerate the packaged mirror, rebuild the client, and re-run `dshe setup` so the embedded bridge is current. Then restart DSH before testing it:

```powershell
node tools/sync-release-assets.mjs
cargo build --release
dshe setup
```

See the [doco index](doco/README.md) for current architecture and contracts, the [readme index](readme/README.md) for operational guides, and the [generated wire reference](doco/specs/wire-protocol.md) for WebSocket framing. The canonical wire source is [`bridge/protocol-contract.json`](bridge/protocol-contract.json).
