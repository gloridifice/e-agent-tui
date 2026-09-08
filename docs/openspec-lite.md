# OpenSpec Lite

> Status: Current
> Authority: Repository planning and specification-delivery workflow. Artifact instructions and templates live in [`openspec/schemas/lite/`](../openspec/schemas/lite/).

The default workflow is `proposal → specs → tasks → implementation/checks → CLI archive`. When requirements stay unchanged, explicitly skip specs: `proposal → tasks`. Lite has no design artifact. Use `spec-driven` for changes needing separate architecture, migration, or security design.

## Plan only the work needed

Reuse the relevant active change for follow-up work; do not create a change for every file or conversation. Pure discussion, reading, and investigation need no change. Read relevant main specs and agreed active deltas before deciding specification impact. A draft does not override agreed requirements.

- Changed or new durable requirements need capability deltas, even for a one-line implementation change. Contracts include compatibility, security, and reliability, not just UI behavior.
- Internal refactors and fixes restoring an unchanged requirement can skip specs. Record the checked capability paths and the reason in the proposal's Impact section.
- Missing existing specs do not automatically justify skipping: a new durable capability may need a new spec. Do not invent requirements for internal implementation details or weaken requirements to accommodate a bug.

For unchanged requirements, add a boolean to the change's `.openspec.yaml`, preserving its schema, creation date, and other fields:

```yaml
skip_specs: true
```

Do not create a `specs/` directory with placeholders or other files for a skipped change. If intended requirements change during implementation, remove the marker, revise the proposal, and add the affected deltas before continuing.

Create a change with the default schema, or explicitly opt into full design:

```powershell
openspec new change my-change
openspec new change redesign-render-pipeline --schema spec-driven
```

Use the propose/apply workflows to draft artifacts and implement tasks. Keep planning proportional: no separate testing, documentation, or process sections just to fill a template. Verification belongs with its implementation task unless it checks broader integration. Before implementation:

```powershell
openspec status --change my-change --json
openspec validate my-change --type change --strict
```

Status reflects artifact availability, not implementation correctness. Run the focused completion checks in tasks according to [testing policy](testing.md).

## Deliver and archive once

Before archiving, inspect task progress and validate the change:

```powershell
openspec instructions apply --change my-change --json
openspec validate my-change --type change --strict
```

Require `state: all_done` and `progress.remaining: 0`, passing relevant completion checks, and agreed deltas. Read diagnostics rather than relying only on exit codes; structural validation cannot prove implementation matches requirements.

Then use the terminal CLI to merge deltas and archive in one path:

```powershell
openspec archive my-change --yes
```

`--yes` bypasses confirmation, including incomplete-task warnings; it does not run tests. Do not use `--skip-specs` or `--no-validate` for normal completion. Metadata `skip_specs: true` means requirements are unchanged; the archive flag `--skip-specs` instead suppresses main-spec synchronization even when deltas exist.

Do not first use a generated sync/archive workflow and then replay the same deltas through CLI archive. Existing changes that have already been synchronized need individual reconciliation, not blind reapplication or batch archiving.

After a behavior change, validate each affected main capability and inspect the files, including untracked additions:

```powershell
openspec validate affected-capability --type spec --strict
git status --short
git diff -- openspec/specs
```

Confirm intended requirements landed and unrelated requirements/scenarios remain intact. Treat implementation, necessary tests, main-spec updates, and archive records as one delivery. If verification or synchronization is deferred, state that explicitly rather than reporting full completion. Concurrent deltas affecting the same requirement should be reconciled against the latest main spec and merged sequentially.

## Configuration maintenance

The local schema preserves the complete OpenSpec 1.12.0 `spec-driven` specs artifact and spec template. Proposal/tasks are shortened, tasks directly require proposal and specs, and apply explicitly requires all three artifacts. Keep artifact rules in the schema, repository-wide policy in [AGENTS.md](../AGENTS.md), and workflow details here; do not duplicate them in config guidance or generated integration files.

When upgrading OpenSpec, compare the installed upstream specs artifact/template before adopting changes. Validate `lite` and test it in a temporary project: explicit skipping succeeds, missing deltas and skip/file conflicts fail validation, tasks alone leave apply blocked, no design is required, and CLI archive preserves unrelated requirements/scenarios while merging a changed requirement.

Schema names are references, not snapshots. Existing changes explicitly using `spec-driven` keep that workflow and their design artifacts; do not convert or archive them merely to adopt Lite. Before editing a schema, inspect change metadata for users of that name. Incompatible schema changes affecting active users should use a new name.

This setup does not change global OpenSpec delivery/profile settings or regenerate local tool integrations. Commands-only delivery is an optional machine-wide choice, not a project schema setting. No one-time patch utility is retained: future maintenance is a direct, reviewed schema edit.
