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
pie --resume <session_id>   # or: pie -r <session_id>
pie --help
```

See the [project README](https://github.com/gloridifice/e#quick-start-pi) for configuration and usage guidance.

## Herdr status

When launched inside a Herdr pane, `pie` automatically reports idle, working, and waiting-for-input states as `pie`. No Pi plugin is required. Herdr decides whether a completed result appears as idle or unseen/done. Reporting is best-effort and does not affect the conversation if Herdr is unavailable.

This integration supports status display only, not automatic session restore after a Herdr server restart. Resume a saved session explicitly with `pie --resume <session_id>` (`-r`) or `pie --session <file>`. On exit, `pie` prints a recovery command for the current saved session.

## License

MIT
