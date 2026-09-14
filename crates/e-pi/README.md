# e-pi

`e-pi` provides `pie`, the experimental Pi RPC adapter for the [`e`](https://github.com/gloridifice/e) terminal UI.

## Install

Install the official Pi runtime and this package:

```text
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi
```

Run `pie` in a project directory. It launches the official `pi --mode rpc` runtime and reuses Pi's native models, credentials, extensions, resources, and sessions. Use `/login [provider]` for native API-key, OAuth, browser, device-code, and multi-step provider setup; use `/logout` to remove a stored native credential.

Authentication uses Pi's public SDK and native credential store. Providers registered by Pi AI, `models.json`, and user extension factories are eligible. Providers registered only later inside the live conversation process, and Pi built-in extension factories not exported through the public SDK, are not listed.

```text
pie
pie --help
```

See the [project README](https://github.com/gloridifice/e#quick-start-pi) for configuration and usage guidance.

## License

MIT
