# Documentation

> Status: Current

This directory contains the maintained documentation for **e** (`dshe` and `pie`), Doco-managed changes, and frozen project history. Documentation is not a mirror of implementation state.

All project documentation is maintained in English.

## Authority

When sources disagree, use this order:

1. approved current contracts under `doco/specs/` and the canonical wire contract at [`bridge/protocol-contract.json`](../bridge/protocol-contract.json);
2. source code, tests, generated contracts/schema, and command `--help` output for exact behavior;
3. Current architecture and methodology documents listed below for stable boundaries, invariants, and rationale;
4. audits, research, migration records, experiments, and archive material as context only.

Historical or archived material never becomes a current requirement by itself. The generated [wire protocol reference](protocol.md) is useful for reading, but its header identifies the machine-readable source that must be edited.

## Current documentation

Start from [current architecture](architecture.md) for implemented boundaries and subsystem routing. Doco is the only workflow for new changes. The entire [`openspec/` tree](../openspec/README.md), including its former main specs, is frozen historical context, not a current authority. Historical requirements become current Doco contracts only after explicit review and approval; do not bulk-promote old specs or treat the transition as a behavior change.

Repository-wide guides:

- [Development](development.md) — checkout installation, build, dependency, and diagnostic entry points.
- [Doco skill](../.agents/skills/doco/SKILL.md) — change creation, execution, review/completion, and explicit archive workflows.
- [Testing and validation](testing.md) — proportional validation policy, scoped tests, protocol checks, and compatibility gates.
- [DSH integration](dsh-integration.md) — profile setup, bridge deployment, restart, and upgrade workflow.
- [Troubleshooting](troubleshooting.md) — operational symptoms and diagnostic routes.
- [Key mappings](key-mapping.md) — scoped keyboard configuration, reload, and terminal limitations.

Subsystem and contract documentation:

- [Rust client](subsystem/client/README.md) — `e-dsh` / `e-tui` ownership, state, interaction, rendering, and runtime invariants.
- [Node.js bridge](subsystem/bridge/README.md) — DSH composition, session, host-integration, and transport invariants.
- [Performance methodology](subsystem/performance/README.md) — repeatable profiling and frame-measurement workflow.
- [Generated wire protocol](protocol.md) — human-readable derivative of the canonical JSON contract.

For user installation, updates, capabilities, and key interactions, see the [root README](../README.md).

## Doco workspace

Use the repository's [Doco skill](../.agents/skills/doco/SKILL.md) for documentation and managed changes. Documentation-only maintenance does not require a change unless tracking is requested.

- `architecture.md` — current system map; detailed invariants stay in the owning subsystem document.
- `specs/` — approved precise current contracts managed by Doco; historical OpenSpec specs have no current authority.
- `decisions/` — durable rationale; superseded decisions must be labeled.
- `changes/` — managed proposals, designs, and tasks; completed and archived changes are not current requirements.
- `tmp/` — ignored scratch material, never a current authority.

## History and archive

- [History](history/README.md) — dated audits, research, migration records, and measurement snapshots; non-normative.
- [Archive](archive/README.md) — superseded design material; frozen and non-authoritative.
- [Retired OpenSpec](../openspec/README.md) — former specifications, changes, schemas, workflow guide, and integrations retained for historical lookup only.

## Update policy

Update a Current document only when a change affects a documented public workflow or interface, architecture boundary or invariant, persistent format or cross-boundary contract, or benchmark methodology. Internal refactors, private renames, mechanically derivable details, and bug fixes that restore an existing contract normally require no documentation change.

Keep each fact in one authoritative location. Prefer source, tests, generated output, schema, or `--help` for exact registries, defaults, field lists, and implementation details. Current documents are maintained in place; audits, reports, experiments, history, and archive are context only. When an old design no longer serves as a concise current reference, freeze it in history or archive and replace it with a smaller Current document instead of continuously synchronizing the old narrative.

Keep the root README concise. Update it only when user-facing installation/build flow, core capabilities, or the keybinding quick reference materially changes; implementation details and development records belong under `doco/`. A user-visible interaction-key change must also update the `e-tui` help overlay and the README quick reference when applicable.
