# e-dsh

`e-dsh` provides `dshe`, the DeepSeek Harness adapter for the [`e`](https://github.com/gloridifice/e) terminal UI.

Windows and PowerShell are currently the primary supported environment.

## Install

Install Git, Rust/Cargo, Node.js/npm, pnpm, and DeepSeek Harness first:

```text
npm install --global pnpm
npm install --global @deepseek-ai/dsh
```

Then install and provision the dedicated DSH profile:

```text
cargo install e-dsh
dshe setup
dshe
```

The executable embeds the tested bridge runtime. Run `dshe setup` again after updating `e-dsh`, then restart any running DSH service.

See the [project README](https://github.com/gloridifice/e#quick-start-dsh) for configuration and interaction guidance.

## License

MIT
