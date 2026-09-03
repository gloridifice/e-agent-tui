# AGENTS.md

Project notes for coding agents. Human readers should see [README.md](README.md). Current architecture
conventions live in [the Rust client documentation](docs/subsystem/client/architecture.md) and
[the Node.js bridge documentation](docs/subsystem/bridge/architecture.md); documentation authority and history
navigation live in [docs/README.md](docs/README.md).

> **Language policy:** All project documentation — this file and everything under `docs/` — is written and
> maintained in **English**. When adding or updating documentation, write English prose; do not introduce new
> Chinese (or other non-English) prose. Code identifiers, file paths, and command names stay as-is.

## What this project is

Terminal frontends for coding-agent runtimes (project name **e**):

- `bridge/` — Node.js (ESM) DSH **host-composition plugin** (one WS upgrade route `/dsh-tui`). Conventions:
  [the bridge architecture](docs/subsystem/bridge/architecture.md).
- `crates/e-dsh/` — Rust DSH adapter and executable package (`e-dsh`, artifact `dshe.exe`; transitional library import name `e`).
- `crates/e-pi/` — Rust Pi RPC adapter and executable package (`e-pi`, artifact `pie.exe`). It launches official `pi --mode rpc`; Pi remains authoritative for credentials, models, resources, extensions, and session writes.
- `crates/e-tui/` — kernel-neutral frontend library package (`e-tui`), owning lifecycle state, projection, rendering, semantic syntax highlighting, paced transcript/Preview text reveal, responsive Preview, Reading View, themes, and config values. Rust conventions: [the client architecture](docs/subsystem/client/architecture.md).

DSH uses JSON WebSocket with the machine-readable contract at `bridge/protocol-contract.json` (see
[docs/protocol.md](docs/protocol.md)); Pi uses strict JSONL over child-process stdio. DSH client config lives at
`%APPDATA%\dshe\`; Pi frontend-only config lives under `%APPDATA%\pie\`. Full overview: [docs/README.md](docs/README.md).

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

# Pi frontend (official Pi runtime must already provide `pi` on PATH)
npm install --global @earendil-works/pi-coding-agent
cargo install --path crates/e-pi --locked
pie                                      # optional: --session <file>, --approve, --no-approve

# Rust workspace (default member e-dsh, artifacts dshe.exe and pie.exe)
cargo run                                # build from root and launch dshe
cargo run -p e-pi --bin pie              # build and launch pie
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

Use [the Rust client architecture](docs/subsystem/client/architecture.md) for frontend/adapter ownership,
projection, rendering, interaction, cache, and runtime invariants. Use
[the bridge architecture](docs/subsystem/bridge/architecture.md) for host composition, connection/session
lifecycle, payload trimming, and DSH integration. Exact wire fields and capacities come only from
`bridge/protocol-contract.json`; [docs/protocol.md](docs/protocol.md) is generated.

## Maintenance discipline

- **Maintain all documentation in English.** AGENTS.md and everything under `docs/` are written in English and
  must be kept in English: write new or edited prose in English, and do not introduce Chinese (or other
  non-English) prose. Code identifiers, file paths, and command names stay as-is.
- For small and medium Rust tasks, do not run `cargo fmt --all` or `cargo clippy` at the end; for large tasks,
  run `cargo fmt --all` and `cargo clippy` at the end. Regardless of task size, `cargo fmt --all` must pass
  before committing.
- Documentation is not a mirror of implementation state; a task completing with no documentation changes is
  normal. Update docs only when a change affects a documented public workflow/interface, architecture boundary/
  invariant, persistent format or cross-boundary ABI, or benchmark methodology. Internal refactors, private
  renames, derivable details, and bug fixes restoring the existing contract normally require no docs edit.
- Keep each fact in one authoritative location. Prefer source, tests, generated output, schema, and `--help` for
  exact behavior. Current docs are maintained; audits, reports, experiments, history, and archive are context only
  and must not constrain current implementation by themselves. Freeze and replace obsolete design narratives
  instead of continuously synchronizing them. See [docs/README.md](docs/README.md) for the full authority and
  lifecycle model.
- User-visible interaction-key changes must still update `e-tui`'s help overlay and the README quick reference
  when applicable.
- **Do not update `README.md` unless necessary, and keep it concise.** Only update README when user-facing basics
  materially change — install/build flow, core user-visible capabilities, or keybinding quick reference;
  implementation details, architecture notes, protocol details, and development records belong in `docs/`, not
  in an expanded README.
- Keep comments minimal. Do not add comments that merely restate what the code does.

## Test discipline

* Add tests cautiously: only when genuinely necessary, when they cover real risk, or when they prevent regressions. Do not add tests merely for formal coverage.
* Unless the user explicitly asks, **do not** run the full `cargo test --lib` or `cargo test`. Only run tests scoped to the module under change, such as `cargo test --lib <module>` or `cargo test <test_name>`. Skip tests entirely for small changes.
* The bridge side uses `node:test` (tests are under `bridge/test/`; run them with `cd bridge && npm test`, using `--test-isolation=none` to avoid `EPERM` when spawning sandboxed processes).
  File-layer tests must use a temporary home directory and must not touch the real `%DSH_HOME%`.
  After protocol changes, also run:
  `node tools/sync-protocol-contract.mjs --check`
  After a DSH upgrade, first mount and run:
  `dsh plugin --profile <p> install`
  Then, from `bridge/`, run:
  `npm run verify-dsh-upgrade`
  Set `DSH_TUI_SMOKE_PROFILE=<p>` if needed.
  This verification smoke-tests public exports, helpers, `/new`, cold resume, and `/model` routing against the actually deployed copy rather than relying on a local waterfall copy.
* Tests must not use external configuration files or theme files.
* Do not add tests for theme styling.

## Known issues

- **Restart DSH to load a new bridge**: after changing `bridge/src`, rebuild the client and run `dshe setup`
  (or re-mount with `.\tools\mount-bridge.ps1 -Profile <web|e>`, equivalent to robocopy) + user restarts dsh.
  The old bridge's startup full disk read is ~14s; the new bridge's active-session path is <100ms.
- The `dshe` binary embeds the bridge runtime and gates startup on a current `.dshe-setup.json` record; a
  missing/stale/damaged setup fails with English guidance to run `dshe setup` before the launcher runs.
- Under `DSH_TUI_TIMING=1`, per-stage startup timings print to stderr, for locating startup regressions.
- Paste accepts terminal bracketed-paste events and an application-owned `Ctrl+V` fallback through the `e-dsh` clipboard-read port. Both paths normalize line endings and target only the active Input Page editor or the visible composer; never let a non-editing Input Page mutate its preserved hidden draft, and never insert modified shortcut letters as text.
- Live assistant Markdown and selected Ready Preview content use presentation-only reveal sidecars; retain complete semantic/copy/cache content, exclude width-dependent fill padding from signatures, and preserve transcript suffix-splice behavior. Transcript admission must reuse UAX #14 wrapping and hold only the unstable trailing atom until a break, timeout, or settlement; Preview pacing applies after wrapping and counts display rows. Compose independent admission, content, and fade deadlines with the spinner clock rather than restoring a fixed ticker.
- Syntax highlighting uses embedded `syntect` grammars through `tui-syntax-highlight`: transcript fences and diff bodies derive token foreground/modifiers from the dedicated `semantics.code` group, Preview Markdown code uses the required `semantics.code_weak` group, and `semantics.diff` retains structural row colors. The `markdown.code_block_bg` role (renamed from the old `code_background`) stays authoritative for code-block/Mermaid fill backgrounds. Keep highlighting bounded (256 KiB/2,000 rows/8 KiB per line), warm syntax assets before the frame loop, cache Preview styled layouts by target/revision/width/theme, and never read files or compute diffs in the client.
