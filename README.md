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

`pie` launches the official `pi --mode rpc` runtime and reuses Pi's native models, credentials, extensions, resources, and session files. Use `pie --session <session.jsonl>` to resume directly; `pie --approve` or `pie --no-approve` explicitly overrides Pi's native project-trust behavior. Run `pie --help` for all launch options.

Type `@` in the composer to browse project-relative paths. Use ↑/↓ to select, Tab or Enter to fill (directories continue browsing), and Esc to dismiss; Enter sends only after file completion closes. Paths containing spaces are quoted automatically.

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

## Config

`e`'s config is under:

- Windows: `%APPDATA%\e`
- Linux: `$XDG_CONFIG_HOME/e` (default `~/.config/e`)
- macOS: `~/.config/e`

You can run `/econfig` in `e` to open or show the config path of your device.

- `config.toml`: Settings configuration. `/econfig` prints its path in the message pane.
- `key_mapping.toml`: Key mapping overrides.
- `themes/`: custom theme folder.

### Key mapping

Bindings are configurable in `<config_path>/key_mapping.toml`; see [key mappings](docs/key-mapping.md) and the complete [defaults](crates/e-tui/assets/default_key_mapping.toml). Below, **Main** means Command on macOS and Ctrl on Windows/Linux (the terminal must forward the shortcut).

In `/model`, **Shift+letter** marks/unmarks the focused model; the plain **letter** switches to it and closes the menu. Marks are saved and shown as Bark-colored ` [a]` suffixes. Letters already mapped in the menu are reserved (by default `h/j/k/l/q`).

Prefix a prompt with `//<mark>` (for example `//i commit`) to use that model for one turn. The Umber model-name preview is not sent. The status-bar model becomes italic; ASAP steering keeps the temporary model, while after-turn messages wait for the original model and reasoning effort to be restored.

Mouse: drag any visible TUI text to copy on release, including input, status, paths, and popups. Multiline selection follows screen rows across both panes; the display pauses during selection while background work continues. A press on the separator resizes instead. Reading View copy still copies the complete source block.

### Themes

Use `/theme` to choose Ferra (default), Rider Dark, Dracula, Catppuccin (Mocha), One Dark, or SynthWave '84. Custom themes are discovered under `<config_path>/themes/<theme_name>.toml`.
See [themes folder](crates/e-tui/assets/themes/) for example.

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

See the [`docs/` index](docs/README.md) for current client and bridge architecture, the generated WebSocket reference, performance methodology, and clearly separated historical material. The canonical wire source is [`bridge/protocol-contract.json`](bridge/protocol-contract.json).
