<!-- doco:managed template=v1 -->
# Execution tasks

Execution was authorized after the user's Chinese catalog revision, with an
additional Settings toggle. The change remains active; no lifecycle transition
is authorized. Blocked: none for implementation. Live acceptance follow-ups are
listed below and keep task 3.1 open.

- [x] 1.1 Review the catalog and close the implementation decisions
  - Design: [selected decisions](implement.md#5-fixed-decisions-and-discretion).
  - Acceptance: Confirm the selected Chinese catalog, English equivalents,
    eligibility sources, API/update semantics, visibility predicate and initial
    interval/width constants. Record decisions before production work; exclude
    held macro candidates unless separately accepted.
  - Verification: Deliver the revised 22 shared, 3 dshe and 8 pie strings with
    English equivalents. Held macro candidates remain excluded. API, availability
    owners, visibility, six-second visible interval, 24-cell minimum budget and
    default-on `show_tips` persistence are recorded in implement.md and source.

- [x] 2.1 Add the shared registry and public registration API
  - Dependencies: 1.1
  - Design: [APIs and data model](implement.md#3-apis-and-data-model).
  - Acceptance: Initialize common definitions once, accept owned adapter batches
    with atomic validation and typed errors, and retain localized semantic segments
    and effective bindings. Satisfy [A3 and A4](specs/composer-tips.md#acceptance-scenarios).
  - Verification: Workspace all-target check passes; both catalog compositions
    initialize in the manual viewer. Source review confirms owned semantic
    segments, typed full-batch rejection before mutation, exact-locale lookup and
    effective binding resolution. Negative external registration cases have not
    been exercised dynamically; see the acceptance follow-ups.

- [x] 2.2 Register each executable's private catalog and availability
  - Dependencies: 2.1
  - Design: [overall approach](implement.md#2-overall-approach).
  - Acceptance: Each adapter registers only its private additions without copying
    shared content. Availability is normalized by existing owners, without renderer
    backend switches or duplicate catalogs. Satisfy [A3 and A4](specs/composer-tips.md#acceptance-scenarios).
  - Verification: Both executables build and register only their own definitions.
    Existing Pi adapter tests pass. Native MCP uses the existing availability fact;
    native auth uses a matching live helper plus the logout catalog entry. Native
    sessionFile supplies saved-session status. No backend branch enters rendering.

- [x] 2.3 Integrate shared work/visibility and deadline-driven rotation
  - Dependencies: 2.1
  - Design: [algorithms and rules](implement.md#4-algorithms-and-rules).
  - Acceptance: Visible reading time drives rotation. Hidden, disabled and idle
    states have no Tips deadline; session replacement resets ephemeral state,
    selection remains immutable and drawing does not advance time. Satisfy
    [A1, A5 and A6](specs/composer-tips.md#acceptance-scenarios).
  - Verification: The manual viewer shows rotation after six visible seconds,
    preserves the selected tip across a seven-second Help pause, and hides on
    idle, disabled settings and Preview-only layout. Existing selection tests
    pass. Reset clears ephemeral rotation/capabilities but retains definitions;
    timers tick outside drawing through the shared presentation deadline path.

- [x] 2.4 Render Tips in the composer's bottom rule and add Settings
  - Dependencies: 2.2, 2.3
  - Design: [target specification](specs/composer-tips.md#placement-and-styling).
  - Acceptance: Right alignment, one-space prefix, semantic colors, existing-label
    priority and grapheme-safe clipping match the prototype without added rows or
    input geometry changes. Settings controls a persistent, live default-on toggle.
    Satisfy [A2 and A6](specs/composer-tips.md#acceptance-scenarios).
  - Verification: Manual PNG and terminal-cell inspection confirms right alignment,
    one-space prefix, Ferra Umber/Bark roles, CJK clipping and unchanged shell label.
    Settings Display shows the localized toggle; turning it off hides Tips, and
    the isolated viewer config serializes `show_tips = false`. Remapping Reading
    to F1 changes its tip; disabling Reading skips it. Existing settings/config
    tests pass. No input height or semantic copy path was changed.

- [ ] 3.1 Finish live acceptance verification
  - Dependencies: 2.4
  - Design: [verification and documentation impact](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Run proportional build/lint and existing checks, inspect acceptance
    scenarios in both frontends, and record results and deferrals. Add no tests
    without authorization. Synchronize delivered architecture, presentation,
    runtime/adapter and config contracts; run `doco check rotating-tips`. Do not
    complete or archive without a separate request.
  - Delivered: Current architecture, presentation, runtime/adapter and config
    contracts are synchronized. README files and tests are unchanged. Doco
    mechanical checks pass; the change remains active.
  - Deferred: Full end-to-end runs against live dshe and native Pi, native auth/MCP
    capability loss/reload and session replacement, negative registration batches,
    missing translations, cross-segment combining/emoji clipping, and Tips-specific
    held-selection/copy checks. The isolated viewer uses production frontend
    rendering/rotation and each catalog but is not a live-adapter acceptance run.
    These scenarios must not be claimed complete from the mock or existing tests.

## Verification

Commands run from the repository root:

- `cargo check --workspace --all-targets` — passed.
- `cargo build --workspace --bins` — passed for dshe and pie.
- `cargo fmt --all` and `cargo fmt --all --check` — passed.
- `cargo clippy --workspace --all-targets` — passed with existing warnings; no
  warning points to the new Tips modules. The stricter exploratory `-D warnings`
  run failed on existing warnings; unrelated lint cleanup is out of scope.
- `cargo test -p e-tui --lib settings` — 16 passed.
- `cargo test -p e-tui --lib config::` — 11 passed.
- `cargo test -p e-tui --lib key_mapping` — 24 passed.
- `cargo test -p e-tui --lib runtime::controller::selection_tests` — 12 passed.
- `cargo test -p e-pi --lib adapter::tests::` — 28 passed.
- `cargo test -p e-pi --lib config::` — 1 passed.
- `cargo test -p e-tui --lib ui::main_pane_tests` — 54 passed, 1 failed:
  `pending_submissions_render_before_any_agent_echo`. The same test failed on an
  unchanged HEAD archive, confirming a pre-existing failure.
- `cargo test -p e-dsh --test architecture` — 14 passed, 2 failed:
  `runners_share_selection_policy_and_publish_only_after_submission` and
  `production_crate_graphs_are_acyclic_and_keep_leaf_boundaries`. Both failures
  reproduce on an unchanged HEAD archive; the latter reports the existing Pi
  extension/MCP/request/response cycle. No tests were changed to bypass failures.
- `git diff --check` — passed.
- `doco check rotating-tips` — mechanical checks passed; the unfinished 3.1 warning
  is intentional.

Manual inspection used a temporary production-library viewer in the ignored
workspace build-output directory, driven by the tui-test Windows CLI. Both private-catalog compositions
were opened; screenshots and cells were inspected at wide and narrow widths.
Only temporary viewer config was written, never the user's frontend config.
The viewer's mock cadence and controls are not production scheduling or new
interaction keys. No test files, snapshots or dependencies were added to the
repository.
