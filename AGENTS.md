# AGENTS.md

Project notes for coding agents. Human readers should see [README.md](README.md); for design decisions see
[docs/design.md](docs/design.md) (D1–D30, protocol, milestones). Detailed architecture conventions live in
[docs/client.md](docs/client.md) (Rust client) and [docs/bridge.md](docs/bridge.md) (Node.js bridge); the full
docs index is in [docs/README.md](docs/README.md).

> **Language policy:** All project documentation — this file and everything under `docs/` — is written and
> maintained in **English**. When adding or updating documentation, write English prose; do not introduce new
> Chinese (or other non-English) prose. Code identifiers, file paths, and command names stay as-is.

## What this project is

Terminal client for DeepSeek Harness (DSH) (project name **e**, executable **`dshe`**), in two parts:

- `bridge/` — Node.js (ESM) DSH **host-composition plugin** (one WS upgrade route `/dsh-tui`). Conventions:
  [docs/bridge.md](docs/bridge.md).
- `crates/e-dsh/` — Rust DSH adapter and executable package (`e-dsh`, artifact `dshe.exe`; transitional library import name `e`).
- `crates/e-tui/` — kernel-neutral frontend library package (`e-tui`), owning lifecycle state, projection, rendering, semantic syntax highlighting, paced transcript/Preview text reveal, responsive Preview, Reading View, themes, and config values. Rust conventions: [docs/client.md](docs/client.md).

The two processes communicate over JSON WebSocket; the machine-readable contract is
`bridge/protocol-contract.json` (see [docs/protocol.md](docs/protocol.md)). Token auth lives at
`%DSH_HOME%\dsh-tui.token`; client config at `%APPDATA%\dshe\config.toml`; themes at `%APPDATA%\dshe\themes\`.
Full overview: [docs/README.md](docs/README.md).

## Common commands (Windows / PowerShell)

The user-facing source install flow is documented in the README "Quick Start": install pnpm and
`@deepseek-ai/dsh` globally, use `cargo install --path crates/e-dsh --locked` to install `dshe.exe` into the
Cargo bin directory, then run `dshe setup` (which embeds the bridge at build time and installs it into the
dedicated `e` profile). Setup checks that pnpm is executable before modifying the profile. On the first updated setup, it moves an existing `profiles\\dshe` directory to `profiles\\e` when `e` is absent; stop/restart DSH around that migration.

```powershell
# First install
npm install --global pnpm
npm install --global @deepseek-ai/dsh
cargo install --path crates/e-dsh --locked
# Embeds and installs the bridge into the dedicated e profile.
# When DSH_HOME is unset/empty, setup falls back to $HOME\.dsh.
dshe setup
# Force-stop only the project-managed DSH service and remove its stale `%DSH_HOME%\e.lock`.
dshe clean

# Rust workspace (default member e-dsh, artifact dshe.exe; e-dsh -> e-tui)
cargo run                                # build from root and launch dshe
cargo build --release                    # artifact target\release\dshe.exe
cargo build --release --features tracy   # Tracy profiling build (activated by DSH_TUI_TRACY=1)
cargo fmt --check
cargo clippy --all-targets
cargo test                               # full unit tests

# Bridge sync (required after changing bridge/; takes effect after restarting dsh)
# Rebuild the client + `dshe setup` to install the re-embedded bridge into the e profile.
# The mount script below remains the fastest dev path to hot-sync bridge/ without a rebuild
# (also used for the `web` profile).
.\tools\mount-bridge.ps1 -Profile web    # or -Profile e (the dshe launcher's dedicated profile)
# Equivalent manual command: robocopy bridge\src "$env:DSH_HOME\profiles\<p>\packages\dsh-tui-bridge\src" /MIR
# The script must be compatible with Windows PowerShell 5.1: empty DSH_HOME falls back to $HOME\.dsh;
# variable names are case-insensitive; Node JSON must be UTF-8 without BOM

# Bridge tests (node:test; includes protocol/session/model-selection edges)
cd bridge; npm test                      # = node --test --test-isolation=none "test/*.test.js"
node tools/sync-protocol-contract.mjs --check
# After a DSH upgrade or bridge change: mount + dsh plugin install (or rebuild + `dshe setup`), then run the full compatibility gate against the deployed copy
$env:DSH_TUI_SMOKE_PROFILE = 'e'; cd bridge; npm run verify-dsh-upgrade

# Integration debugging
node tools/probe-online.mjs       # is the bridge online
node tools/hello-test.mjs         # send hello and print all frames (verify the startup path)
node tools/probe-startup.mjs      # attach latency / snapshot size
node tools/dump-snapshot.mjs      # capture a snapshot sample -> tools/cache/snapshot-sample.json
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json
cargo run --release --example timing_frames # 1002-message continuous scroll/stream/animation frame benchmark
cargo run --release --example timing_syntax # syntax cold-start/cache/stream/diff benchmark
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

cargo uses the official crates.io registry (local network is fixed). `crates/e-dsh/vendor/` and
`tools/vendor-crates.mjs` are legacy offline fallbacks, now retired — do not depend on them again; add new
dependencies directly to the owning package manifest (`crates/e-dsh/Cargo.toml` for DSH/infrastructure, `crates/e-tui/Cargo.toml` for frontend values/rendering) and commit the root `Cargo.lock` workspace lockfile.

## Key architecture conventions

Implementation conventions are documented per part and are the source of truth when editing:

- **client (Rust)** — [docs/client.md](docs/client.md): event display model (four public surfaces), reasoning
  folding, file/tool activity formatting (including reserved trailing metrics), surface semantics, render cache,
  performance red lines, runtime/lock discipline, input & character boundaries, overlays/Input Page (including
  ask_user_question pages that suppress tool activity and preserve the input draft), semantic Reading/copy,
  responsive Preview and deferred resolution, application-owned visible mouse selection/copy alongside wheel scrolling, layered rendering, markdown/table styling, history paging,
  status bar, command paradigm, deferred `/new`, stable transcript grapheme reveal, Preview row reveal/fade and independent animation deadlines,
  config/theme/launcher.
- **bridge (Node.js)** — [docs/bridge.md](docs/bridge.md): module layout, DSH command integration, dynamic
  user-question relay, cross-await conn discipline, snapshot/history data sources, payload trimming,
  protocol-contract sync, `/new` workspace
  inheritance, session title, model-selection install, `/login` `/model` `/skill` bridging.

## Maintenance discipline

- **Maintain all documentation in English.** AGENTS.md and everything under `docs/` are written in English and
  must be kept in English: write new or edited prose in English, and do not introduce Chinese (or other
  non-English) prose. Code identifiers, file paths, and command names stay as-is.
- For small and medium Rust tasks, do not run `cargo fmt --all` or `cargo clippy` at the end; for large tasks,
  run `cargo fmt --all` and `cargo clippy` at the end. Regardless of task size, `cargo fmt --all` must pass
  before committing.
- After a task, sync this file (AGENTS.md) and related `docs/` (e.g. design.md, client.md, bridge.md) to the
  scope of the change; descriptions of features, interaction keys, protocol fields, config defaults, or command
  lists must not lag. Key changes must also sync `ui.rs`'s `help_overlay`.
- **Do not update `README.md` unless necessary, and keep it concise.** Only update README when user-facing
  basics materially change — install/build flow, core user-visible capabilities, or keybinding quick reference;
  implementation details, architecture notes, protocol details, and development records belong in `docs/`, not
  in an expanded README.

## Test discipline

- Add tests cautiously: only when genuinely necessary, when they cover real risk or prevent regression; do not
  add tests for formal coverage's sake.
- Unless the user explicitly asks or the change is a large-scale refactor, do **not** run the full
  `cargo test --lib` or `cargo test`; only run `cargo test` scoped to the module under change (e.g.
  `cargo test --lib <module>` or `cargo test <test_name>`). Skip tests entirely for small changes.
  Rendering/spacing changes must have UI-layer regression tests (TestBackend asserting cached line
  counts/colors/content), not only model-layer tests.
- Known flake: full parallel tests occasionally flake once (tool card assertion); a single or rerun passes — do
  not make big changes based on it.
- The bridge side has `node:test` (`bridge/test/`, `cd bridge && npm test`, using `--test-isolation=none` to
  avoid sandbox spawn EPERM): besides trim/compose/login/skill/model/model-selection,
  host/connection/history/session/session-list/protocol/dispatcher edges must also be covered; file-layer tests
  use a temp home (do not touch the real `%DSH_HOME%`). Protocol changes must also run
  `node tools/sync-protocol-contract.mjs --check`. After a DSH upgrade, mount + `dsh plugin --profile <p>
  install`, then run `npm run verify-dsh-upgrade` from `bridge/` (set `DSH_TUI_SMOKE_PROFILE=<p>` if needed);
  it smokes public exports, helpers, `/new`, cold resume, and `/model` routing against the deployed copy rather
  than relying on a local waterfall copy.

## Known issues

- **Restart DSH to load a new bridge**: after changing `bridge/src`, rebuild the client and run `dshe setup`
  (or re-mount with `.\tools\mount-bridge.ps1 -Profile <web|e>`, equivalent to robocopy) + user restarts dsh.
  The old bridge's startup full disk read is ~14s; the new bridge's active-session path is <100ms.
- The `dshe` binary embeds the bridge runtime and gates startup on a current `.dshe-setup.json` record; a
  missing/stale/damaged setup fails with English guidance to run `dshe setup` before the launcher runs.
- Design-doc M milestone numbering has fallen behind the implementation (features exceed M6); code and README
  are authoritative.
- Under `DSH_TUI_TIMING=1`, per-stage startup timings print to stderr, for locating startup regressions.
- Live assistant Markdown and selected Ready Preview content use presentation-only reveal sidecars; retain complete semantic/copy/cache content, exclude width-dependent fill padding from signatures, and preserve transcript suffix-splice behavior. Transcript admission must reuse UAX #14 wrapping and hold only the unstable trailing atom until a break, timeout, or settlement; Preview pacing applies after wrapping and counts display rows. Compose independent admission, content, and fade deadlines with the spinner clock rather than restoring a fixed ticker.
- Syntax highlighting uses embedded `syntect` grammars through `tui-syntax-highlight`: transcript fences and diff bodies derive token foreground/modifiers from `semantics.markdown`, Preview Markdown uses required `semantics.markdown_weak`, and `semantics.diff` retains structural row colors. Keep highlighting bounded (256 KiB/2,000 rows/8 KiB per line), warm syntax assets before the frame loop, cache Preview styled layouts by target/revision/width/theme, and never read files or compute diffs in the client.
