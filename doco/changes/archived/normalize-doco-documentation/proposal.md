# Normalize Doco documentation

## Purpose

The previous migration renamed the documentation directory but retained a second subsystem architecture tree, mixed human guides with current contracts, left specs and decisions empty, and linked historical material into the default context graph. Reorganize by Doco document responsibility, not by the old folder layout.

## Scope and acceptance

- Maintain one implemented architecture entry, focused current contracts under specs, and durable documentation-policy rationale under decisions.
- Preserve user/developer procedures under readme and old audits/designs as explicitly historical material outside the current Doco graph. Retain OpenSpec as history only.
- Preserve existing application requirements; do not import former OpenSpec specs as current facts or change application behavior.
- Update generated protocol location, generator, consumer test, source comment, mirror, and all affected live links.
- Verify the selected Doco package mechanically, local links, default context isolation, generated protocol and bridge mirror consistency, and the focused protocol test.
- Do not refresh installed Doco templates or modify the Doco CLI. Completion and commit were explicitly requested by the user after implementation; archiving was not requested.

## Result

Completed. Doco now holds one current architecture entry, six focused contracts, one durable ownership decision, and this tracked package; the former subsystem, history, and archive trees moved out of Doco.

Delivered:

- `doco/architecture.md` is the single implemented architecture entry; `doco/specs/` holds runtime/adapter, presentation, interaction/session, configuration/storage, DSH bridge, and generated wire-protocol contracts; `doco/decisions/0001-document-ownership.md` records document ownership and the OpenSpec retirement boundary.
- 25 documents moved to `readme/`: 8 byte-identical and the remainder changed only for link targets and Historical labels when compared with commit `a219feb`. No document content was lost.
- The generated protocol reference moved to `doco/specs/wire-protocol.md`; generator, bridge comment, bridge test, Cargo comment, `AGENTS.md`, root `README.md`, fixtures, and the embedded bridge mirror were updated and regenerated.
- Application behavior, protocol payloads, and OpenSpec history were left unchanged; former OpenSpec specs were not promoted into current contracts.

Adjustment: the initial scope excluded completion and commit. Both were explicitly requested afterwards and were performed. Archiving remains undone.

Verification conclusion: `doco check` passed, `doco context` returned only current paths with zero history/archive/OpenSpec/tmp candidates, 119 local links resolved with 0 missing, the protocol and release-mirror `--check` modes passed, the focused bridge protocol test passed, and `git diff --check` reported no whitespace errors. Rust builds, the full bridge suite, and the deployed-copy DSH gate were not run and are outside this change's acceptance criteria; anchors and historical wording were reviewed by inspection only.
