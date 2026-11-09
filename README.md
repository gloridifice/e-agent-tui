<div align="center">

<img src="readme/logo.png" alt="e logo" width="128">

# e

</div>

<div align="center">

English | [简体中文](readme/README.zh-CN.md) | [日本語](readme/README.ja-JP.md) | [Français](readme/README.fr-FR.md)

</div>

`e` is a terminal UI for coding agents: `dshe` connects to [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness), and `pie` runs Pi through its official RPC interface, without replacing either agent runtime.

> Early-stage software. Windows and PowerShell are the primary supported environment; prebuilt binaries are also available for Linux x64.

https://github.com/user-attachments/assets/45a75c55-7e3b-4af3-b1e2-1c6306b80358

## Quick Start

### Install

Choose the frontend for your agent. The commands below require [Node.js and npm](https://nodejs.org/) and [Rust with Cargo](https://rustup.rs/). DSH also requires [Git](https://git-scm.com/) and pnpm.

**DeepSeek Harness — `dshe`**

```bash
npm install --global pnpm @deepseek-ai/dsh
cargo install e-dsh --locked
dshe setup
```

**Pi — `pie`**

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi --locked
```

Prefer not to install Rust? Download a Windows x64 ZIP or Linux x64 tarball from [GitHub Releases](https://github.com/gloridifice/e/releases), verify it against `SHA256SUMS`, and extract the executables into a directory on `PATH`. Each archive includes both frontends. Install your chosen agent's npm dependencies above; DSH still needs Git and `dshe setup`.

Run `dshe` or `pie` in your project directory, then use `/login` to configure access to your provider. `dshe` starts its dedicated DSH service automatically; `pie` reuses Pi's native credentials, models, extensions, and session files.

Use `/help` inside either frontend for keyboard controls, and `pie --help` for Pi launch options. See [DSH setup and upgrades](doco/guides/dsh-integration.md) and [Pi runtime requirements](doco/guides/development.md#prerequisites) for compatibility details.

### Update

Re-run the npm and Cargo install commands above for your chosen frontend. For prebuilt installs, replace the executables with those from the latest release.

After updating DSH or `dshe`, run `dshe setup` again and restart any running DSH service so it loads the updated bridge. `/reload` does not replace that restart.

## Features

- Streaming Markdown with syntax-highlighted code, tables, Mermaid diagrams, and diff previews.
- Resizable Preview pane and Reading mode for inspecting messages and tool output.
- Multiline editing, input history, command and path completion, and queued prompts.
- Model and reasoning-effort selection, reusable prompt macros, and a configurable compaction model.
- Pi-native authentication, MCP management, and session resume, fork, and clone workflows.
- Local shell commands kept out of model context and agent history; they are noninteractive and run with your permissions.
- Execution history with operation timings and token/cost timelines. Exported metadata may contain sensitive command arguments.
- Custom themes, configurable key bindings, and English or Simplified Chinese interface text.

## Config

Use `/settings` for common preferences or `/econfig` to locate the shared frontend configuration:

- Windows: `%APPDATA%\e`
- Linux: `$XDG_CONFIG_HOME/e` (default `~/.config/e`)
- macOS: `~/.config/e`

`config.toml` stores settings; set `language = "zh-CN"` for Simplified Chinese. English is the default and fallback. Agent credentials and session files remain managed by DSH or Pi.

Customize `key_mapping.toml` with the [key mapping guide](doco/guides/key-mapping.md) and [default bindings](crates/e-tui/assets/default_key_mapping.toml). Add theme TOML files under `themes/`, using the [bundled themes](crates/e-tui/assets/themes/) as examples. `/reload` refreshes configuration, themes, and backend resources.

## Build from Source

Requires Git and Rust/Cargo. The runtime prerequisites above still apply.

```bash
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release --locked -p e-dsh -p e-pi --bins
```

The executables are in `target/release/`. For checkout installs, bridge changes, and profiling builds, see the [development guide](doco/guides/development.md).

## License

Licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

## Development

Issues are welcome. Pull requests are not being accepted yet while the project is a prototype.

- [Documentation index](doco/README.md)
- [Development guide](doco/guides/development.md)
- [Architecture](doco/architecture.md)
- [Testing and validation](doco/guides/testing.md)
- [Troubleshooting](doco/guides/troubleshooting.md)
