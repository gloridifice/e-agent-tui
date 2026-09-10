<!-- doco:managed template=v1 -->
# Review or complete

Review the selected active change against its goal, approved design and actual
source/contracts. Verify all in-scope tasks really completed, agreed acceptance
has actual evidence, no unresolved blockers remain, and affected architecture,
specs and durable decisions have been synchronized with delivered facts.

If an environment cannot run required acceptance, report the limitation. Do not
count missing evidence as success unless the user explicitly accepts the stated
limitation or approves revised acceptance. Record that decision and the actual
verification performed; a checkbox/confirmation flag does not prove correctness.

Update proposal Result with delivered scope, material adjustments and verification
conclusion. Keep it concise and independently readable without work/. Do not copy
implementation detail or task logs into the historical summary.

Run `doco check <id>`. Only when semantic review is complete and completion was
requested, run `doco complete <id>`. CLI success means mechanical checks and the
whole-directory move succeeded; it does not certify semantic acceptance. completed/
retains the complete work package as a delivery snapshot, not current truth.
Completion does not commit, merge or publish and does not authorize archiving.

If corrections are needed later, use `doco reopen <id>` with approval, then reset
or add tasks and verify again. Keep previously independent valid evidence.
