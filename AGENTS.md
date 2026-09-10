# AGENTS.md

Project guidance for coding agents. Human-facing documentation starts at [README.md](README.md); the documentation map and authority rules live in [doco/README.md](doco/README.md).

> All project documentation is maintained in English. Do not add non-English prose to this file or `doco/`.

## Project map

- `bridge/` — Node.js DSH host-composition plugin.
- `crates/e-dsh/` — DSH adapter and `dshe` executable.
- `crates/e-pi/` — Pi RPC adapter and `pie` executable.
- `crates/e-tui/` — runtime-neutral TUI frontend.

DSH uses JSON WebSocket; Pi uses JSONL over child-process stdio. The canonical DSH wire contract is [`bridge/protocol-contract.json`](bridge/protocol-contract.json); [`doco/protocol.md`](doco/protocol.md) is generated.

## Architecture routing

Start from [current architecture](doco/architecture.md). Before changing a subsystem, read its current architecture document:

- Rust adapters, frontend, rendering, interaction, cache, and runtime invariants: [client architecture](doco/subsystem/client/architecture.md).
- Bridge composition, connection/session lifecycle, trimming, and DSH host integration: [bridge architecture](doco/subsystem/bridge/architecture.md).

Changes under `bridge/` also require the deployment lifecycle in [DSH integration](doco/dsh-integration.md). Exact fields, defaults, registries, and capacities come from source, tests, schema, generated output, or `--help`, not prose summaries.

## Working policy

- Follow the owning subsystem's boundaries and keep each fact in one authoritative location.
- Documentation is not an implementation mirror. No documentation change is normal for internal refactors, private renames, derivable details, and bug fixes that restore an existing contract.
- Update documentation only for a documented public workflow/interface, architecture boundary/invariant, persistent format or cross-boundary ABI, or benchmark methodology. Full policy: [doco/README.md](doco/README.md).
- Keep `README.md` concise and user-facing; do not add implementation detail there.
- When changing user-visible interaction keys, update the `e-tui` help overlay and the README quick reference when applicable.
- Keep comments minimal; do not add comments that merely restate code.
- Follow [development guidance](doco/development.md) for dependency ownership and repository commands.

## Change policy

All new changes use Doco. For code changes, reuse a relevant active Doco change
or create one following the `doco` skill. Keep planning proportional and perform
only the requested phase; implementation does not authorize completion or archive.
Discussion, investigation, and documentation-only maintenance do not require a
change unless tracking is requested.

Read relevant current contracts under `doco/specs/` and effective decisions before
changing behavior. Drafts and historical records do not override current contracts.
Do not invent or weaken requirements to satisfy validation or accommodate a bug.
Synchronize delivered contract and architecture changes into current Doco documents
and run relevant checks before reporting completion; explicitly report deferrals.

`openspec/` is deprecated, frozen historical material, including its former specs,
changes, schemas, and integrations. Do not create, execute, validate, or archive
new work with OpenSpec. Consult it only when historical context is explicitly
needed; it is not a current specification source.

## Validation

Choose validation proportionally to the change and read [doco/testing.md](doco/testing.md) before broad checks.

- Add tests only for real risk or regression prevention, not formal coverage.
- Prefer scoped Rust tests. Do not run `cargo test --lib` or `cargo test` by default; trivial changes may need no tests.
- Bridge tests use `node:test`; protocol changes also require the generated-contract check.
- Large Rust changes require workspace formatting and Clippy; small and medium changes do not require broad end-of-task checks.

## Task guides

- Build, install, dependency, debug, and performance entry points: [doco/development.md](doco/development.md)
- Test and validation policy: [doco/testing.md](doco/testing.md)
- DSH profiles, setup, bridge deployment, and upgrades: [doco/dsh-integration.md](doco/dsh-integration.md)
- Operational diagnosis: [doco/troubleshooting.md](doco/troubleshooting.md)

<!-- DOCO:START -->
## Doco

For project documentation and managed changes, use the `doco` skill.
Read `.agents/skills/doco/SKILL.md` before creating, executing, completing,
or archiving a change. Perform only the requested phase.
Start from `doco/architecture.md` and relevant current specs and decisions.
For implementation, use the selected active change's proposal, design,
and tasks. Treat completed changes, archived changes, and `doco/tmp/`
as non-current material; consult them only when explicitly needed.
<!-- DOCO:END -->
