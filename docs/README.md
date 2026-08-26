# Documentation

> Status: Current

This directory contains the maintained architecture and measurement method for **e** / `dshe`, plus frozen project history. Documentation is not a mirror of implementation state.

All project documentation is maintained in English.

## Authority

When sources disagree, use this order:

1. active normative specifications under `openspec/specs/` and the canonical wire contract at [`bridge/protocol-contract.json`](../bridge/protocol-contract.json);
2. source code, tests, generated contracts/schema, and command `--help` output for exact behavior;
3. Current architecture and methodology documents listed below for stable boundaries, invariants, and rationale;
4. audits, research, migration records, experiments, and archive material as context only.

Historical or archived material never becomes a current requirement by itself. The generated [wire protocol reference](protocol.md) is useful for reading, but its header identifies the machine-readable source that must be edited.

## Current documentation

- [Rust client](subsystem/client/README.md) — `e-dsh` / `e-tui` ownership, state, interaction, rendering, and runtime invariants.
- [Node.js bridge](subsystem/bridge/README.md) — DSH composition, session, host-integration, and transport invariants.
- [Performance methodology](subsystem/performance/README.md) — repeatable profiling and frame-measurement workflow.
- [Generated wire protocol](protocol.md) — human-readable derivative of the canonical JSON contract.

For installation, common commands, and user-facing key interactions, see the [root README](../README.md).

## History and archive

- [History](history/README.md) — dated audits, research, migration records, and measurement snapshots; non-normative.
- [Archive](archive/README.md) — superseded design material; frozen and non-authoritative.

## Update policy

Update a Current document only when a change affects a documented public workflow or interface, architecture boundary or invariant, persistent format or cross-boundary contract, or benchmark methodology. Internal refactors, private renames, mechanically derivable details, and bug fixes that restore an existing contract normally require no documentation change.

Keep each fact in one authoritative location. Prefer source, tests, generated output, schema, or `--help` for exact registries, defaults, field lists, and implementation details. When an old design no longer serves as a concise current reference, freeze it in history or archive and replace it with a smaller Current document instead of continuously synchronizing the old narrative.
