<!-- doco:managed template=v1 -->
# Create

1. Decide whether the user requested implementation/change tracking, not merely
   discussion or investigation. Run `doco list`; reuse an active ID for the same
   goal instead of fragmenting follow-up fixes and verification into new changes.
2. Read current architecture, relevant specs, effective ADRs and actual source
   and tests. Exclude unrelated active changes, completed snapshots, archived
   history and tmp unless explicitly needed. CLI context is a path aid, not a
   semantic relevance engine.
3. For a new goal, agree on a unique lowercase-hyphen ID and run `doco new <id>`.
   Templates under `../templates/` are skeletons, not a finished design. Replace
   every TODO placeholder. Small changes may be short but require all three files.
4. Write proposal: problem, motivation, scope, non-goals, acceptance criteria and
   intended contract changes; set Result to Pending until delivered. It must
   remain understandable after all of work/ is deleted.
5. Write work/implement.md: actual baseline and affected source entry points;
   distinguish existing from planned APIs. Cover module boundaries, call/data
   flow, API signatures, error semantics, state ownership, lifetime, persistence,
   concurrency and compatibility where relevant. Specify algorithm steps,
   invariants, edge cases, failure handling and resource/performance constraints.
   A Git commit is only a locator; also account for uncommitted relevant changes.
6. Close correctness/architecture/API/core-algorithm decisions before handing off.
   State fixed choices versus local discretion, blocking open questions,
   verification and current-document impact (explicitly none when appropriate).
7. Write work/tasks.md: `- [ ] 1.1 action`, stable unique two-level positive-integer
   IDs (`1.1`, `1.2`, `2.1`; no zero or leading zero), Acceptance lines,
   Dependencies lines where needed, and a final verification task. Dependencies
   must exist and be acyclic. Include document updates only if actually affected.
   Use relative package links; cross-change references use `doco:<stable-id>`.
8. Run `doco check <id>` and resolve mechanical errors. Review semantic design
   completeness yourself: another implementer must not have to invent key choices.

Generate optional visual HTML only on request in `doco/tmp/<id>/`; it is a
throwaway explanation, never requirements or execution state.
