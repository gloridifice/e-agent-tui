# ADR-0001: Separate current contracts from guides and history

Status: Accepted

## Context

The former documentation tree mixed architecture, precise behavior, operational procedures, generated output, and historical records. Renaming that tree to `doco/` made Doco context broad and left `specs/` and `decisions/` without owners.

## Decision

- `doco/architecture.md` is the single current architecture entry.
- `doco/specs/` contains approved precise contracts and generated contract references.
- `doco/decisions/` contains durable rationale, not duplicated requirements.
- `doco/changes/` contains managed lifecycle packages; only selected active work is executable.
- `readme/` contains current operational guides plus clearly separated history and archive material.
- `openspec/` is frozen history. It is not imported into current contracts without explicit review and approval.
- Current Doco documents do not link historical Markdown into the default context graph.

## Consequences

Doco context stays focused on current facts and selected work. Historical detail remains available by deliberate lookup. Moving a document does not promote or demote application behavior; contract changes still require explicit approval.
