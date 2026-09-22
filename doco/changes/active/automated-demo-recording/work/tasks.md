# Execution tasks

Discussion draft. This checklist is a candidate sequence, not authorization to implement. Only creation of this change package has been requested; all tasks remain pending.

- [ ] 1.1 Resolve the recording design with the user
  - Design: [candidate design](implement.md), [decision register Q1–Q6](../proposal.md#open-decisions).
  - Acceptance: Confirm timing strategy, effort semantics and supported level, model routes, scenario content, visual settings, execution/isolation policy, and finite failure budgets. Update the design and this checklist, then obtain implementation authorization before proceeding.

- [ ] 2.1 Build and inspect a minimal real-application recording
  - Dependencies: 1.1
  - Design: [baseline and approach](implement.md#1-baseline-and-goals).
  - Acceptance: In the approved isolated environment, drive the real `pie` through the selected task, verify required input sequences and completion signals, and inspect a playable video for working/Reading visuals. Confirm the preferred route rather than silently replacing it.

- [ ] 2.2 Implement the approved five-feature recording and export workflow
  - Dependencies: 2.1
  - Design: [interfaces](implement.md#3-apis-and-data-model), [execution rules](implement.md#4-algorithms-and-rules).
  - Acceptance: The approved entry point generates the five-scene video using the agreed state transitions, time/cost limits, edit policy, artifact handling, and cleanup. Temporary-route restoration and native fork/resume are visibly demonstrated.

- [ ] 3.1 Verify the final artifact and review current-document impact
  - Dependencies: 2.2
  - Design: [verification and documentation](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Check video encoding and duration at or below 90 seconds; inspect every feature scene, readability, timing disclosures, privacy, and cleanup. Document a delivered public workflow only where applicable. Report limitations without marking the change complete or archiving it unless separately requested.

## Verification

- `doco check automated-demo-recording`: passed mechanical validation, with the expected warning that tasks 1.1, 2.1, 2.2, and 3.1 are unfinished. This does not certify implementation readiness or semantic acceptance.
- Only the change-package creation phase is in scope. No application implementation, new model invocation, or full demo recording has been performed for this package.
