# AGENTS.md

Project guidance for coding agents. Human-facing documentation starts at [README.md](README.md); the documentation map and authority rules live in [docs/README.md](docs/README.md).

> All project documentation is maintained in English. Do not add non-English prose to this file or `docs/`.

## Project map

- `bridge/` — Node.js DSH host-composition plugin.
- `crates/e-dsh/` — DSH adapter and `dshe` executable.
- `crates/e-pi/` — Pi RPC adapter and `pie` executable.
- `crates/e-tui/` — runtime-neutral TUI frontend.

DSH uses JSON WebSocket; Pi uses JSONL over child-process stdio. The canonical DSH wire contract is [`bridge/protocol-contract.json`](bridge/protocol-contract.json); [`docs/protocol.md`](docs/protocol.md) is generated.

## Architecture routing

Before changing a subsystem, read its current architecture document:

- Rust adapters, frontend, rendering, interaction, cache, and runtime invariants: [client architecture](docs/subsystem/client/architecture.md).
- Bridge composition, connection/session lifecycle, trimming, and DSH host integration: [bridge architecture](docs/subsystem/bridge/architecture.md).

Changes under `bridge/` also require the deployment lifecycle in [DSH integration](docs/dsh-integration.md). Exact fields, defaults, registries, and capacities come from source, tests, schema, generated output, or `--help`, not prose summaries.

## Working policy

- Follow the owning subsystem's boundaries and keep each fact in one authoritative location.
- Documentation is not an implementation mirror. No documentation change is normal for internal refactors, private renames, derivable details, and bug fixes that restore an existing contract.
- Update documentation only for a documented public workflow/interface, architecture boundary/invariant, persistent format or cross-boundary ABI, or benchmark methodology. Full policy: [docs/README.md](docs/README.md).
- Keep `README.md` concise and user-facing; do not add implementation detail there.
- When changing user-visible interaction keys, update the `e-tui` help overlay and the README quick reference when applicable.
- Keep comments minimal; do not add comments that merely restate code.
- Follow [development guidance](docs/development.md) for dependency ownership and repository commands.

## OpenSpec

For code changes, reuse the relevant active change or create one with `lite`.
Read relevant main specs and agreed active deltas before changing their behavior;
drafts do not override agreed requirements. Use `skip_specs: true` only when
requirements stay unchanged, including fixes that restore specified behavior.
Do not invent or weaken requirements to satisfy validation or accommodate a bug.
Before reporting a behavior change as complete, run the relevant checks and merge
its deltas into main specs. If that is intentionally deferred, report it explicitly.
Keep planning proportional; use `spec-driven` when a separate design is needed.
Discussion and investigation alone do not require a change.
Workflow and CLI archive checks: [OpenSpec Lite](docs/openspec-lite.md) (read when needed).

## Validation

Choose validation proportionally to the change and read [docs/testing.md](docs/testing.md) before broad checks.

- Add tests only for real risk or regression prevention, not formal coverage.
- Prefer scoped Rust tests. Do not run `cargo test --lib` or `cargo test` by default; trivial changes may need no tests.
- Bridge tests use `node:test`; protocol changes also require the generated-contract check.
- Large Rust changes require workspace formatting and Clippy; small and medium changes do not require broad end-of-task checks.

## Task guides

- Build, install, dependency, debug, and performance entry points: [docs/development.md](docs/development.md)
- Test and validation policy: [docs/testing.md](docs/testing.md)
- DSH profiles, setup, bridge deployment, and upgrades: [docs/dsh-integration.md](docs/dsh-integration.md)
- Operational diagnosis: [docs/troubleshooting.md](docs/troubleshooting.md)
