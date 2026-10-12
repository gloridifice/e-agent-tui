# e-pi

`e-pi` provides `pie`, the experimental Pi RPC adapter for the [`e`](https://github.com/gloridifice/e) terminal UI.

## Install

Install the official Pi runtime and this package:

```text
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi
```

Run `pie` in a project directory. It launches the official `pi --mode rpc` runtime and reuses Pi's native models, credentials, extensions, resources, and sessions.

```text
pie
pie --resume <session_id>   # or: pie -r <session_id>
pie --help
```

See the [project README](https://github.com/gloridifice/e#quick-start-pi) for quick start and configuration, and the [Pi workflows guide](https://github.com/gloridifice/e/blob/master/doco/guides/development.md#pi-workflows) for authentication, fork/clone, resume, and Herdr status. Detailed project documentation is maintained in [Doco](https://github.com/gloridifice/e/blob/master/doco/README.md).

## License

MIT
