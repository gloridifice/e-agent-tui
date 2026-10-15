# Execution tasks

Execution is in progress. Core schema, dispatch, completion, feedback, and adapter normalization have been restored and merged onto `98eaa1b`, including the latest Tips implementation, but acceptance remains incomplete. Tasks stay unchecked until their full acceptance is verified. The user subsequently authorized repairing the five failing checks and focused coverage for their fixes; completion and archive remain unauthorized.

- [ ] 1.1 Add the pure macro schema, exact matcher, and complete/partial definition parser.
  - Design: [schema and parsing](implement.md#pure-schema-and-parsing).
  - Acceptance: [R1/R2](specs/prompt-macros.md#r1-definition-and-storage) and [S1/S2](specs/prompt-macros.md#s1-definition-and-persistence): validated array with empty embedded default, same-ID replacement, independently optional fields, original-body preservation, exact raw-buffer matching before newline normalization, and nonrecursive expansion. No filesystem work or second config owner in shared code.

- [ ] 1.2 Register <code>/prompt-macro</code> and connect live config updates and adapter persistence.
  - Dependencies: 1.1
  - Design: [confirmation and persistence](implement.md#confirmation-and-persistence).
  - Acceptance: [S1](specs/prompt-macros.md#s1-definition-and-persistence): valid definitions become discoverable, invalid ones preserve the old definition, reload/restart reads the shared format, and failed disk writes are reported as unsaved without backend execution or changes to backend/default-model configuration.

- [ ] 2.1 Introduce owned macro preparation and reuse the ordinary command/prompt dispatch paths.
  - Dependencies: 1.1
  - Design: [overall approach](implement.md#2-overall-approach) and [prepared submissions](implement.md#prepared-submissions-and-runtime-ownership).
  - Acceptance: [S2](specs/prompt-macros.md#s2-exact-input), [S4](specs/prompt-macros.md#s4-commands-and-drafts): exact IDs expand once; original invocation is recoverable; queued bodies are snapshots; direct commands stay direct, shell stays local, and both skill forms retain their accompanying messages including after <code>/new</code>. Extract shared outcome application rather than duplicating terminal logic or fabricating model marks. Test changes require user authorization.

- [ ] 2.2 Extend shared temporary routing and normalize the required adapter settlement events.
  - The [approved Record revision](implement.md#7-approved-record-lifecycle-revision) is implemented: both adapters provide correlated dispatch-time fences, and consumed steering owners transfer one baseline A. Live A → B → C → A, rejection/cancellation, and deployed-DSH verification remain deferred; existing checks alone do not establish this acceptance.
  - Dependencies: 2.1
  - Design: [routing, commands, and failure](implement.md#routing-commands-and-failure).
  - Acceptance: [S3-S5](specs/prompt-macros.md#s3-route-combinations-and-isolation): all option combinations, target capture, explicit effort confirmation, after-turn isolation, command-without-run settlement, exact restoration, cancellation/failure recovery, and stale-session rejection. Contradictory controls fail before effects; native acceptance alone never ends a run. Existing marked-prompt steering and adapter ownership remain intact; no macro-specific external wire messages.

- [ ] 3.1 Implement cursor-aware definition, embedded-command, and bare-ID completion.
  - Dependencies: 1.2, 2.1
  - Design: [completion](implement.md#completion).
  - Acceptance: [S6](specs/prompt-macros.md#s6-completion-without-accidental-execution): IDs/options/models/target-dependent efforts/skills and existing body path completion work with preserved suffixes and atomic blocks. Enter cannot execute an implicit fuzzy candidate or submit a definition while accepting a suggestion. Esc, remapped actions, and reload/catalog refresh keep their documented semantics.

- [ ] 3.2 Render exact-match feedback and the one-row expanded-body hint.
  - Dependencies: 1.1, 2.1
  - Design: [one-row preview](implement.md#one-row-preview).
  - Acceptance: [S7](specs/prompt-macros.md#s7-one-row-feedback-and-unicode): Coral ID, existing-style bottom `macro`, and Umber body hint activate together. Clip the first logical line to remaining cells with one trailing `…` when content is hidden, preserving graphemes and real cursor/IME positions. No wrapping, extra composer height, buffer mutation, hidden source loss, render-time I/O, or new theme-styling tests.

- [ ] 4.1 Synchronize delivered public contracts and frontend help/hints.
  - Dependencies: 1.2, 2.2, 3.1, 3.2
  - Design: [verification and documentation impact](implement.md#6-verification-and-documentation-impact).
  - Acceptance: [current-document impact](specs/prompt-macros.md#current-document-impact): current interaction, presentation, configuration, and architecture describe only delivered behavior; any new runtime settlement invariant is recorded. The unsupported keybinding-macro restriction remains intact. Preserve the root README, unrelated change packages, and this package's pending lifecycle until separately authorized.

- [ ] 4.2 Run scoped verification and review the delivered feature against every acceptance scenario.
  - Dependencies: 4.1
  - Design: [verification plan](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Record actual commands/results, scoped test outcomes and authorized repairs, manual exercises of [S1-S7](specs/prompt-macros.md#acceptance-scenarios), cross-cutting formatting/Clippy, and `doco check prompt-macros`. Explicitly report unavailable backends/models, unexercised failures or terminal families, and any remaining work. Do not add or modify tests without user authorization; do not claim completion or archive as part of execution.

## Verification

Creation-stage inspection: `doco list` found no same-goal active change; `git rev-parse --short HEAD` returned `825b5a9`; `git status --short` was clean before creation. Relevant current contracts, source, and selected existing behavior tests were read. `doco new prompt-macros` created the full package.

Creation validation: `doco check prompt-macros` passed mechanical checks with only the expected warning for eight unfinished tasks. Its initial pass classified inline slash-command notation as absolute file references; explicit HTML code spans and relative source links corrected the notation without changing requirements. `git diff --check` reported no tracked whitespace errors. A package-local Python validation also checked all four new Markdown files for whitespace and resolved all 59 relative links/anchors successfully.

### Implementation checks

| Actual command | Result |
| --- | --- |
| `cargo fmt --all`; `cargo check --workspace` | Passed. |
| `cargo clippy --all-targets`; `cargo clippy --workspace --all-targets` | Passed with warnings; this is not a warning-free result. |
| `cargo build -p e-pi -p e-dsh --bins` | Passed; debug binaries built, not installed. |
| `cargo test -p e-tui --lib runtime::controller::model_tests` | 13 passed. |
| `cargo test -p e-tui --lib runtime::state::session` | 4 passed. |
| `cargo test -p e-tui --lib ui::input::tests` | 3 passed. |
| `cargo test -p e-tui --lib runtime::command` | 11 passed. |
| `cargo test -p e-tui --lib model_defaults` | 2 passed. |
| `cargo test -p e-tui --lib config` | 18 passed. |
| `cargo test -p e-tui --lib help` | 8 passed. |
| `cargo test -p e-pi --lib adapter::model` | 3 passed. |
| `cargo test -p e-pi --lib adapter::queue` | 6 passed. |
| `cargo test -p e-pi --lib skill` | 9 passed, including existing/new-session joint skill-message submission and cancellation. |
| `cargo test -p e-pi --lib command` | 3 passed. |
| `cargo test -p e-tui --lib input` | 149 passed, 1 failed: the unchanged fixed roster expectation omits the new <code>/prompt-macro</code> candidate. No ranking workaround or test edit was made. |
| `cargo test -p e-pi --lib adapter::reload_tests` | 4 passed, 1 failed at `reload_tests.rs:39`. The same failure reproduced in the clean baseline with <code>cargo test --manifest-path /tmp/prompt-macros-baseline/Cargo.toml -p e-pi --lib adapter::reload_tests::reload_uses_native_companion_then_replaces_catalog_before_releasing_work</code>. |
| `cargo test -p e-dsh --test architecture` | 14 passed, 2 failed: `runners_share_selection_policy_and_publish_only_after_submission` and `production_crate_graphs_are_acyclic_and_keep_leaf_boundaries`. Both reproduced in the baseline; the reported Pi strongly connected component contains `extension`, `mcp`, `request`, and `response`. |

Final repository checks: `cargo fmt --all --check` and `git diff --check` passed using the repository's normal line-ending configuration. `doco check prompt-macros` passed mechanical checks with warnings for eight unfinished tasks and the unresolved task 2.2 blocker; it does not certify acceptance. Existing inline test tails were compared against HEAD in nine changed Rust files and remained identical; dedicated test files and the root README remained unchanged.

The scoped commands overlap and are not a unique-test total. Existing tests do not constitute complete macro acceptance. No tests or snapshots were added, removed, or edited. No bridge production/protocol or root README changes were made.

### Restoration and merge verification

Recovered the successful implementation edits from the original local session against `825b5a9`; both surviving new source files matched the reconstructed versions. Merged onto `98eaa1b` without reverting the Tips changes. Composer rendering and Tips scheduling now share the same status-label width calculation, reserving the macro label before clipping Tips. Task 2.2's boundary blocker remains unresolved; restoration does not close acceptance or change lifecycle status.

Checks rerun after merging:

- `cargo fmt --all --check`, `cargo check --workspace`, and `cargo build -p e-pi -p e-dsh --bins`: passed; debug binaries built, not installed.
- `cargo clippy --workspace --all-targets`: passed with warnings, not warning-free.
- `cargo test -p e-tui --lib` with individual filters `runtime::command`, `runtime::controller::model_tests`, `runtime::state::session`, `config`, `model_defaults`, and `help`: passed (11, 13, 4, 18, 2, and 8 respectively).
- `cargo test -p e-pi --lib` with individual filters `adapter::queue`, `adapter::model`, `skill`, and `command`: passed (6, 3, 9, and 3 respectively).
- `cargo test -p e-tui --lib input`: 149 passed, 1 failed. The unchanged fuzzy roster assertion still omits the newly registered command; neither ranking nor tests were changed to hide this mismatch.
- `cargo test -p e-tui --lib ui::main_pane_tests`: 54 passed, 1 failed (`pending_submissions_render_before_any_agent_echo`).
- `cargo test -p e-pi --lib adapter::reload_tests`: 4 passed, 1 failed (the existing native-companion roster assertion).
- `cargo test -p e-dsh --test architecture`: 14 passed, the same 2 failures listed above.

The main-pane failure, reload failure, and both architecture failures were reproduced in a clean `git archive 98eaa1b` extraction with the corresponding Cargo manifest and selectors. They are baseline failures, not new merge failures. All nine existing inline test tails in changed Rust files remain identical to HEAD; no dedicated tests, snapshots, bridge files, root README, or unrelated change packages were modified. The original manual evidence below was not rerun; no new real-backend route, macro/Tips visual coexistence, or physical-terminal acceptance is claimed.

### Record lifecycle revision verification

The approved revision supersedes the earlier boundary-blocker notes. Records now retain prompt/state snapshots through local queues and adapter deferral. The first dispatch captures A behind a correlated adapter fence: Pi reads native state after pending controls; DSH serializes model refresh/pong and correlates it inside the socket adapter. Generic frontend heartbeats cannot capture a baseline. Consumed ASAP successors inherit A; unconfigured successors use A rather than B. Native dispatch/admission IDs, whole-skill consumption, command settlement, queue clear, and restoration remain owned barriers. Pi Record identity also survives new-session materialization. Interruption holds an owned post-abort state fence, including Pi abort acknowledgment, rather than waiting forever for a cancelled prompt's missing echo or releasing on cached idle. DSH refresh callbacks reject detached connections, and direct command outcomes publish current host/inbox status first.

Verification after the final Record changes:

- `cargo fmt --all --check`, `cargo check --workspace`, and `cargo build -p e-pi -p e-dsh --bins`: passed; no installation.
- `cargo clippy --workspace --all-targets`: passed with warnings, not warning-free.
- Existing e-tui selectors `runtime::controller::model_tests`, `runtime::command`, and `interaction`: 13, 11, and 8 passed.
- Existing e-pi selectors `adapter::queue`, `adapter::compaction`, `skill`, and `command`: 6, 7, 9, and 3 passed. A separate `retry` selector matched zero tests and supplies no retry verification.
- Existing e-dsh selectors `bridge::adapter` and binary `queued_dispatch_releases_the_state_lock`: 16 and 1 passed.
- <code>node tools/sync-release-assets.mjs --check</code>, <code>node tools/sync-protocol-contract.mjs --check</code>, and bridge `npm test`: passed; 113 bridge tests passed. Generated package metadata needed LF normalization; no wire fields or protocol version changed.
- `doco check prompt-macros`: mechanical checks passed with eight unfinished-task warnings. This does not certify semantic acceptance.
- `git diff --check`: passed. Dedicated tests, snapshots, root README, and unrelated change packages remain unchanged; all 14 existing inline test tails in changed Rust files match HEAD.

The known failing selectors were also rerun during this revision: e-tui `input` (149 passed, one roster expectation omitting the new command), `ui::main_pane_tests` (54 passed, the existing pending-submission failure), Pi `adapter::reload_tests` (4 passed, the existing roster failure), and DSH `architecture` (14 passed, the same two baseline failures). No tests or production ranking were changed to conceal them.

No real-backend A → B → C → A exercise or new physical-terminal matrix was run. The DSH deployed-copy gate requires installation/restart, which remains unauthorized. Existing checks do not close the native route/failure acceptance gaps below; tasks remain unchecked, active, and unarchived.

### Authorized failing-check repairs

All five reported failures were reproduced before editing. The fuzzy-completion check now exercises ranked popup behavior with independent command fixtures instead of copying the growing built-in roster. Pending-submission rendering uses the current lowercase `[skill]` identity; no missing-card production bug was found. Reload checks companion-command hiding rather than incorrectly requiring an empty catalog after native fork/clone commands were added.

The frame architecture guard no longer prescribes `?` rather than Pi's cleanup-preserving `match`: both runners now use a shared submission-checked presentation commit. Its existing owner-boundary test exercises errors before and after rendering, preserves the held screen/selection on failure, and commits on success. The architecture check still forbids bypassing that shared gate.

The cycle check itself is unchanged. MCP follow-up scheduling/deferred draining now belongs to Pi's outer dispatcher, not the MCP handler. Once that cycle was removed, the same check exposed a previously masked frontend cycle; moving unchanged MCP message definitions into a pure leaf and keeping the existing public re-exports removed it. Two focused adapter tests protect reload priority, delayed release, timeout release, and no late replay. Negative controls that published a failed frame or omitted MCP draining failed the intended assertions; those mutations were reverted.

Scoped validation: e-tui `input` (150), `ui::main_pane_tests` (55), `ui::selection::tests` (5), `runtime::controller::selection_tests` (12), and `page_core` (5) passed. Pi `adapter::reload_tests` (7), `adapter::queue` (6), `adapter::compaction` (7), and architecture (2) passed; DSH architecture (16) passed, including the unchanged whole-workspace cycle/leaf checks. Counts overlap and are not a unique-test total. Terminal transaction tests (4) also passed after restoring all negative controls. Workspace all-target checks with `--locked`, formatting, generated-asset checks, and Clippy passed (existing warnings remain); Doco passed with the same eight unfinished tasks. No unfiltered Rust suite was run. This repairs the five failures, not the outstanding real-backend macro acceptance gaps below. No installation, commit, completion, or archive was performed.

### Acceptance evidence and deferrals

| Scenario | Evidence / remaining work |
| --- | --- |
| S1 | Manual TUI rejected invalid IDs and empty bodies with specific diagnostics. Persistence/restart, same-ID replacement, and isolated write-failure exercises remain unverified. |
| S2 | Raw-buffer matcher and single-expansion path implemented. The complete exact/nonmatch, pasted, image, and nonrecursive execution matrix remains unverified. |
| S3 | Existing routing/default-effort checks pass. The approved dispatch-time fences and Record takeover are implemented; native all-option/isolation and A → B → C → A exercises remain unverified. |
| S4 | Existing Pi skill/new-session/cancellation checks pass. Actual macro-origin skill execution on both backends and routed no-run commands remain unverified. |
| S5 | Correlated admissions, no-run settlement, uncertain-state holds, and restoration barriers implemented. Real rejection/cancellation/restoration-failure/stale-callback matrix remains unverified. |
| S6 | Manual TUI showed `--model`, `--effort`, `--`, body <code>/help</code> and <code>/theme</code> candidates; `@sub` offered <code>subdir/</code> and Tab filled it. Model/effort candidates, fuzzy Enter, suffix/Esc, and reload matrix remain unverified. |
| S7 | Existing raw-cursor/image/model-hint renderer checks pass. Exact macro styling and the CJK/combining/ZWJ/multiline/resize width matrix remain unverified. |

Manual runs encountered `execution-history is already in use or cannot be locked`; a later Pi prompt was rejected with `No API key found for the selected model`. No successful real-model macro run is claimed. No physical Windows Terminal/ConHost/macOS/Linux terminal-family compatibility matrix was run. The boundary decision is approved and implemented; a usable isolated backend/configuration and authorized DSH deployment are still required before closing these acceptance gaps. Lifecycle remains active and incomplete.
