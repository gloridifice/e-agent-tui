<!-- doco:managed template=v1 -->
# Execution tasks

All production tasks remain unstarted. Creating this package records the
current draft; it does not authorize implementation or completion.

- [ ] 1.1 Review the catalog and close the implementation decisions
  - Design: [fixed decisions and pending review](implement.md#5-fixed-decisions-and-discretion).
  - Acceptance: Confirm the initial [Chinese catalog](catalog.zh-CN.md), English
    translations, eligibility sources, API/update semantics, visibility predicate
    and interval/width constants. Record the decisions in implement.md before
    production work. Held macro candidates are excluded unless separately accepted.

- [ ] 2.1 Add the shared registry and public registration API
  - Dependencies: 1.1
  - Design: [APIs and data model](implement.md#3-apis-and-data-model).
  - Acceptance: e-tui initializes common definitions once, accepts adapter batches,
    validates atomically and returns typed errors. Localized styled segments and
    effective bindings retain their semantic roles. Satisfy [A3 and A4](specs/composer-tips.md#acceptance-scenarios).

- [ ] 2.2 Register each executable's private catalog and availability
  - Dependencies: 2.1
  - Design: [overall approach](implement.md#2-overall-approach).
  - Acceptance: e-dsh registers dshe-only additions and e-pi registers pie-only
    additions without copying shared content. Live prerequisites are normalized
    by their owners; no renderer backend switches or duplicated catalogs are added.
    Satisfy [A3 and A4](specs/composer-tips.md#acceptance-scenarios).

- [ ] 2.3 Integrate shared work/visibility and deadline-driven rotation
  - Dependencies: 2.1
  - Design: [algorithms and rules](implement.md#4-algorithms-and-rules).
  - Acceptance: Visible reading time drives periodic changes; hidden and idle
    states have no Tips deadline. Session replacement resets ephemeral state,
    selection stays immutable and drawing does not advance time. Satisfy
    [A1, A5 and A6](specs/composer-tips.md#acceptance-scenarios).

- [ ] 2.4 Render Tips in the composer's bottom rule
  - Dependencies: 2.2, 2.3
  - Design: [target specification](specs/composer-tips.md#placement-and-styling).
  - Acceptance: Right alignment, one-space label separation, semantic colors,
    existing-label priority and grapheme-safe clipping match the approved
    prototype without adding rows or altering input geometry. Satisfy
    [A2 and A6](specs/composer-tips.md#acceptance-scenarios).

- [ ] 3.1 Verify integration and synchronize delivered contracts
  - Dependencies: 2.4
  - Design: [verification and documentation impact](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Run proportional build/lint and existing scoped checks, inspect
    the scenarios in both frontends, and record commands/results and deferrals.
    Add no tests without separate authorization. Update current architecture,
    presentation and runtime/adapter contracts only for delivered approved facts;
    update config/storage only if a persistent interface changes. Run
    `doco check rotating-tips`. Leave lifecycle transitions to a separate request.

## Verification

Preparation evidence only; none of the production acceptance scenarios is
claimed complete.

- The earlier mock run used tui-test to inspect working/idle transitions,
  automatic rotation, Chinese/English content, shell labels, higher-priority
  status, narrow width and multiline input. Terminal-cell inspection confirmed
  Ferra Umber/Bark foregrounds. Those results validate a visual mock, not the
  planned registry or production scheduling.
- Before packaging, the latest one-space prefix source passed:

  ```powershell
  python -m py_compile target/prototype/tips.py
  ```

- A preparation-only Python check compiled the retained source in memory and
  confirmed it differs from the reviewed prototype only in its documented run
  path. It resolved local links in all five Markdown files, verified 37 unique
  candidate IDs (35 plus 2 held), and confirmed that packaging preserved every
  Chinese UI string, including the user's revised send wording.
- `doco check rotating-tips` passed its mechanical checks. All six production
  tasks remain unchecked; that warning is intentional for this active draft.
- Rust builds/tests and production validation have not run: no production code
  was changed by this request.
