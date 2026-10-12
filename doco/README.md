# Current documentation

All detailed project documentation is maintained here. The root and crate READMEs are concise public entry points, not separate documentation stores.

## Architecture and contracts

- [Architecture](architecture.md) — implemented boundaries, dependencies, state, and data flow.
- [Runtime and adapters](specs/runtime-and-adapters.md) — provider-neutral runtime and adapter contracts.
- [Presentation](specs/presentation.md) — event surfaces, layout, rendering, Preview, reveal, and copy contracts.
- [Interaction and sessions](specs/interaction-and-sessions.md) — input, pages, queues, commands, and session contracts.
- [Configuration and storage](specs/configuration-and-storage.md) — config, key mapping, themes, and execution-history contracts.
- [DSH bridge](specs/dsh-bridge.md) — host composition, lifecycle, framing, trimming, and deployment contracts.
- [Wire protocol](specs/wire-protocol.md) — generated derivative of `bridge/protocol-contract.json`.
- [Decisions](decisions/) — durable rationale that is not an application contract.

Exact registries, defaults, payload fields, and CLI options come from source, tests, generated artifacts, schema, or `--help`.

## Guides

- [Development](guides/development.md) — checkout install, build, dependencies, release, Pi workflows, and diagnostic entry points.
- [Testing and validation](guides/testing.md) — scoped checks, protocol verification, DSH upgrades, and terminal compatibility.
- [DSH integration](guides/dsh-integration.md) — profile setup, bridge deployment, restart, and upgrades.
- [Troubleshooting](guides/troubleshooting.md) — operational symptoms and diagnostic routes.
- [Key mappings](guides/key-mapping.md) — customization, reload, model marks, link copy, and terminal limitations.
- [Performance measurement](guides/performance.md) — repeatable profiling and frame measurement.
- [Automated Pi demo recording](guides/demo-recording.md) — release-binary recording, model selection, safety, and artifact checks.

Guides explain how to operate and validate the project. They link to contracts rather than maintaining a second set of requirements.

## Authority

When facts disagree, use this order:

1. approved contracts under `doco/specs/` and canonical machine-readable contracts;
2. source, tests, generated artifacts, schema, and command help for exact behavior;
3. `doco/architecture.md` and effective decisions for stable boundaries and rationale;
4. guides for procedures and examples.

A code/spec disagreement must be investigated. Do not rewrite a contract to hide a defect. Draft changes and completed or archived change packages never override current contracts.

## Ownership

- `doco/` owns current architecture, specs, decisions, guides, and managed change packages. This index is the single navigation entry.
- `readme/` contains only README assets and translations named `README.<locale>.md`. Keep assets at stable paths; translations follow the root README and do not define independent behavior.
- Root and crate READMEs remain brief public/package entry points. Licenses and Agent tooling instructions retain their required locations.
- Delete obsolete documentation instead of moving it into history or archive directories. Git retains prior versions. OpenSpec is retired and must not be recreated.
- Doco change packages follow their own lifecycle. Documentation cleanup does not authorize completing, cancelling, archiving, or deleting a change package.

`AGENTS.md`, the root and crate READMEs, and Doco documents are maintained in English. README translations use their target languages.

Use the project Doco skill for changes. Documentation-only maintenance does not require a change unless tracking is requested. Update current documents only for public workflows or interfaces, architecture boundaries, persistent formats, cross-boundary contracts, or measurement methodology.
