# docs

Design and architecture documentation for the **e** / `dshe` project. Everything here — and in
[AGENTS.md](../AGENTS.md) — is written and maintained in English (see the language policy there).

## Index

- [client.md](client.md) — Rust TUI client architecture conventions (event display model, reasoning/folding,
  render cache, performance red lines, input/character boundaries, overlays/Input Page, command paradigm,
  deferred `/new`, config/theme/launcher).
- [bridge.md](bridge.md) — Node.js bridge architecture conventions (module layout, DSH command integration,
  cross-await conn discipline, snapshot/history data sources, payload trimming, `/new` workspace inheritance,
  session title, model-selection install, `/login` `/model` `/skill` bridging).
- [design.md](design.md) — design decisions D1–D30, protocol, milestones.
- [protocol.md](protocol.md) — generated protocol documentation (hand-written source of truth is
  `bridge/protocol-contract.json`).
- [tracy.md](tracy.md) — Tracy profiling notes.
- [architecture-audit.md](architecture-audit.md) — architecture audit notes.
- [plan/](plan/README.md): planned Rust workspace refactor, agent-kernel boundary, unified Preview pane,
  Reading View, runtime model, and phased migration. These documents describe future work, not current behavior.

## Project overview

Terminal client for DeepSeek Harness (DSH) (project name **e**, executable **`dshe`**), in two parts:

- `bridge/` — Node.js (ESM) DSH **host-composition plugin**. Registers one WS upgrade route
  (`/dsh-tui`), forwards session events to the TUI, and accepts input/commands/interrupt/approval answers/
  session switching/history paging, plus `/login` `/model` `/skill:<name>` bridging. The only injected
  dependency is `webServer`.
- `crates/e-dsh/` — Rust DSH adapter and executable package (`e-dsh`, artifact `dshe.exe`; transitional library import name `e`). It currently includes the launcher, protocol/setup infrastructure, and legacy UI modules while extraction proceeds.
- `crates/e-tui/` — kernel-neutral frontend library package (`e-tui`). The package boundary exists now; normalized contracts and UI ownership move into it phase by phase.

The two processes communicate over JSON WebSocket; the only machine-readable contract is
`bridge/protocol-contract.json` (the sole hand-written source of truth for version/capacities/roster/
shapeTypes/records/messageShapes). `tools/sync-protocol-contract.mjs` syncs `docs/protocol.md`,
`crates/e-dsh/build.rs` constants/shape JSON, Rust/Node conformance fixtures, and
`bridge/package.json.dshCompatibility.wireProtocol` from it; `--check` must pass after changing the contract.
`bridge/src/protocol.js` reads the same JSON at runtime; `tools/generate-protocol-doc.mjs` is only a
compatibility wrapper. Token auth; the token lives at `%DSH_HOME%\dsh-tui.token`.
Client config lives at `%APPDATA%\dshe\config.toml`; the default config source is
`crates/e-tui/assets/default_config.toml` (embedded via `include_str!` and parsed; the user TOML only overrides
known keys and is then deserialized through a single strict `Config` schema; missing fields inherit,
deprecated unknown keys are ignored, malformed/known-type errors fall back safely); themes live in
`%APPDATA%\dshe\themes\`.
