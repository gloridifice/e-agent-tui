# Current documentation

- [Architecture](architecture.md) — implemented boundaries, dependencies, state, and data flow.
- [Runtime and adapters](specs/runtime-and-adapters.md) — provider-neutral runtime and adapter contracts.
- [Presentation](specs/presentation.md) — event surfaces, layout, rendering, Preview, reveal, and copy contracts.
- [Interaction and sessions](specs/interaction-and-sessions.md) — input, pages, queues, commands, and session contracts.
- [Configuration and storage](specs/configuration-and-storage.md) — config, key mapping, themes, and execution-history contracts.
- [DSH bridge](specs/dsh-bridge.md) — host composition, lifecycle, framing, trimming, and deployment contracts.
- [Wire protocol](specs/wire-protocol.md) — generated derivative of `bridge/protocol-contract.json`.
- [Decisions](decisions/) — durable rationale that is not an application contract.

Exact registries, defaults, payload fields, and CLI options come from source, tests, generated artifacts, schema, or `--help`.

## Authority

When facts disagree, use this order:

1. approved contracts under `doco/specs/` and canonical machine-readable contracts;
2. source, tests, generated artifacts, schema, and command help for exact behavior;
3. `doco/architecture.md` and effective decisions for stable boundaries and rationale;
4. operational guides and historical records as context only.

A code/spec disagreement must be investigated. Do not rewrite a contract to hide a defect. Draft changes and historical records never override current contracts.

## Ownership

Doco contains only current architecture, specs, effective decisions, and managed change packages. Operational guides live under `readme/`. Historical audits, designs, and superseded documentation live under `readme/history/` or `readme/archive/`. Retired OpenSpec material remains under `openspec/` and has no current authority.

Use the project Doco skill for changes. Documentation-only maintenance does not require a change unless tracking is requested. Update current documents only for public workflows or interfaces, architecture boundaries, persistent formats, cross-boundary contracts, or measurement methodology.
