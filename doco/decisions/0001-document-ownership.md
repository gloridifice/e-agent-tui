# ADR-0001: Maintain project documentation in Doco

Status: Accepted

## Context

Separating architecture and contracts from operational guides left two documentation entry points. Keeping old audits, designs, and measurements in the working tree also made superseded material easy to mistake for current guidance. README assets and translations have a different audience from detailed project documentation.

## Decision

- `doco/README.md` is the single detailed-documentation index.
- `doco/architecture.md` records implemented boundaries and data flow.
- `doco/specs/` contains approved contracts and generated contract references.
- `doco/decisions/` contains durable rationale, not duplicated requirements.
- `doco/guides/` contains user, development, deployment, validation, troubleshooting, and measurement procedures. Guides link to the owning contracts.
- `doco/changes/` contains managed lifecycle packages; only selected active work is executable. Documentation cleanup does not change their lifecycle status.
- `readme/` contains only README assets and translations. Root and crate READMEs remain concise public entry points; licenses and Agent tooling instructions remain in their required locations.
- Obsolete documentation is deleted, not moved to history or archive directories. Git preserves earlier versions. The retired OpenSpec tree is removed and must not be recreated.
- Canonical documentation remains English; README translations use their target languages and do not become independent contract sources.

## Consequences

Current facts and procedures are reachable from one index without mixing their authority. README assets keep stable paths, and translations track the root README. Moving a document does not change application behavior; contract changes still require explicit approval. Historical context requires deliberate lookup in Git rather than a second documentation tree.
