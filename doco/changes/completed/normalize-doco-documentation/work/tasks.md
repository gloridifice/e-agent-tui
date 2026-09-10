# Execution tasks

- [x] 1.1 Classify current documentation and verify owning boundaries
  - Acceptance: Doco rules and the existing application documentation are inspected; retained contracts match source and tests; historical requirements are not imported.
  - Verification: Read the installed Doco skill, its format reference, and the context resolver. Cross-checked retained facts against runtime policy/ports, Pi JSONL framing, the Rust architecture test, and the bridge connection, session, prompt, frame, trim, model, and protocol tests. Former OpenSpec specs were treated as history only.

- [x] 1.2 Separate architecture, specs, decisions, guides, and history
  - Dependencies: 1.1
  - Acceptance: One current architecture, focused specs, and a durable ownership decision exist under Doco; guides and historical snapshots move out of the current tree; no document content is lost.
  - Verification: Added the single architecture entry, six contracts under specs, and one decision. Relocated 25 documents out of Doco into the operational readme tree: 8 byte-identical, the rest edited only for link targets and Historical labels when compared with commit `a219feb`. The former subsystem, history, and archive trees under Doco were removed.

- [x] 1.3 Update protocol generation and affected references
  - Dependencies: 1.2
  - Acceptance: The generated protocol reference and all of its consumers use the new Doco location.
  - Verification: Regenerated the protocol reference and both fixture files, then regenerated the embedded bridge mirror. Generator, bridge comment, bridge test, Cargo comment, AGENTS guide, and root README use the new paths.

- [x] 2.1 Verify structure, context isolation, links, and generated output
  - Dependencies: 1.3
  - Acceptance: Doco check, context isolation, local links, generated-file checks, the focused protocol test, and whitespace checks pass; results and limits are recorded; the package stays active.
  - Verification: See below. All executed checks passed.

## Verification

```text
doco check normalize-doco-documentation      # mechanical checks pass
doco context normalize-doco-documentation    # 13 current paths, no history/archive/openspec/tmp
node tools/sync-protocol-contract.mjs --check
node tools/sync-release-assets.mjs --check
node --test --test-isolation=none bridge/test/protocol.test.js
git diff --check
```

- Local-link resolution over the current documentation trees reported 119 targets checked and 0 missing after fixing one relative path.
- Context isolation improved from a pre-change list that pulled historical and archived files to the current set with none.
- Not run: Rust builds and tests, the full bridge suite, and the deployed-copy DSH upgrade gate. This change is documentation and path metadata only, so those gates are outside its acceptance criteria.
- Limitation: anchors and historical-record wording were reviewed by inspection; no external link checker or renderer ran.
