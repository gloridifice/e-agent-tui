## 1. Baseline and migration controls

- [x] 1.1 Record the scoped commands and current results for runtime, projection, transcript layout, UI, setup, launcher, wire-conformance, and architecture tests.
- [x] 1.2 Run and record the current formatting, scoped Clippy, protocol-contract, and workspace build baselines before moving files.
- [x] 1.3 Capture release-mode `timing_snapshot` and `timing_frames` results on the documented reference environment.
- [x] 1.4 Add or capture TestBackend characterization fixtures for the current transcript surfaces, composer/Input Pages, status/title rows, spacing, colors, and hidden cursor at representative widths.
- [x] 1.5 Record the current source-install, `cargo run`, setup bridge digest, and launcher behavior so the package move has explicit compatibility checks.

## 2. Create the two-package workspace

- [x] 2.1 Create `crates/e-dsh` and `crates/e-tui`, update the root workspace/default member, and keep the binary artifact named `dshe`.
- [x] 2.2 Move the current Rust package, `src/lib.rs`, `src/main.rs`, tests, examples, and `build.rs` into `crates/e-dsh` without changing runtime behavior.
- [x] 2.3 Repair build-script and runtime paths to `bridge/protocol-contract.json`, embedded `bridge/src`, generated constants, examples, and test fixtures.
- [x] 2.4 Add the minimal `e-tui` library and the one-way path dependency from `e-dsh`.
- [x] 2.5 Adapt architecture/source scanners and test discovery to scan both package roots while preserving the existing SCC checks.
- [x] 2.6 Verify root `cargo run`, `cargo install --path crates/e-dsh --locked`, launcher/setup tests, embedded bridge digest, and executable startup with no visible UI change.
- [x] 2.7 Decide whether a one-release `client` source-install compatibility path is required and implement/document it without duplicating the package if required.

## 3. Establish kernel-neutral contracts

- [x] 3.1 Move UI config/default and theme value schemas plus bundled assets into `e-tui`, while keeping config paths, file persistence, discovery, and state-file I/O in `e-dsh`.
- [x] 3.2 Define public `AgentEvent` families and frontend `InputEvent` values in `e-tui` without importing DSH types.
- [x] 3.3 Define `UiAction`, `UpdateResult`, dirty-state, deadline, effect-result, and Preview request/completion value types with owned payloads, including the now-owned `Config` snapshot.
- [x] 3.4 Define normalized tool capabilities, activity state, tool Items, and namespaced custom presentation escape hatches.
- [x] 3.5 Implement the `e-dsh` adapter from DSH server frames and typed protocol events to normalized `AgentEvent` values.
- [x] 3.6 Implement the `e-dsh` adapter from agent-facing `UiAction` requests to existing DSH `ClientMessage` shapes.
- [x] 3.7 Adapt the existing `e-dsh` runtime controller as a transitional state owner that consumes normalized `AgentEvent` values and returns `UiAction` values; move the synchronous update owner into `e-tui` with the UI core in the following phases.
- [x] 3.8 Add deterministic contract tests for normalized projection, outbound mapping, complete action payloads, protocol mismatch handling, and guard release before effects.
- [x] 3.9 Add architecture guards that reject DSH protocol/WebSocket imports and raw DSH event names inside `e-tui`.

## 4. Extract the existing UI core without visual change

- [x] 4.1 Introduce `e-tui::TimelineModel` as the sole owner of display surfaces, transcript storage, and normalized projection-family state; keep `e-dsh::AppState` only as a temporary forwarding facade until Phase 5.
- [x] 4.2 Move composer/input state, command catalog presentation, completion, approval/question routing, and Input Page controllers into `e-tui`.
- [x] 4.3 Move transcript-layout, Markdown/Mermaid, atomic-provenance, and render-cache primitives/state into `e-tui`; preserve the tail-splice and targeted-patch APIs while their renderer integration moves with the UI in 4.5.
- [x] 4.4 Move copy selection/payload logic into `e-tui` after its display/layout dependencies, while keeping system clipboard implementation in the `e-dsh` action executor.
- [x] 4.5 Forward the `tracy` feature from `e-dsh` to `e-tui` and preserve no-op behavior when profiling is inactive.
- [ ] 4.6 Repair scoped model, projection, input, copy/provenance, cache, and runtime tests after each non-rendering ownership move.

## 5. Decompose application state by lifecycle

- [ ] 5.1 Introduce `TuiApp` and `UpdateResult` entry points while retaining temporary forwarding methods needed by unmigrated callers.
- [ ] 5.2 Extract attached session, deferred-new, history-page, title/workspace, model/mode, status, and usage state into `SessionModel` as the sole owner.
- [ ] 5.3 Extract provider/model/command/skill and related presentation catalogs into `CatalogModel` as the sole owner.
- [ ] 5.4 Extract composer, scroll/follow, focus, Input Page, approval, help, queue, and notice state into `InteractionModel` as the sole owner.
- [ ] 5.5 Complete transfer of remaining timeline correlations and semantic annotation inputs from the temporary `AppState` facade into the existing `TimelineModel`, without introducing a mirror transcript.
- [ ] 5.6 Extract width registries, render caches, animation transitions, and copy provenance into `RenderState` without storing domain text twice.
- [ ] 5.7 Add state-transition tests for snapshot replay, history prepend, replacement/shadowing, cross-page correlation, retry enrichment, deferred `/new`, queue dispatch, and question-draft restoration.
- [ ] 5.8 Add source guards for sole ownership and remove migrated compatibility fields rather than leaving synchronized mirror state.

## 6. Extract and reorganize rendering while preserving the main style

- [ ] 6.1 After the lifecycle owners in Phase 5 exist, move theme resolution and the existing single-column Ratatui UI, including tail-splice and targeted activity-patch integration, into `e-tui`; leave terminal lifecycle and frame orchestration in `e-dsh`.
- [ ] 6.2 Re-run characterization UI tests and release timing scenarios to verify the ownership move has not changed the current screen or crossed performance redlines.
- [ ] 6.3 Create leaf Component modules for the current working indicator, card shell, Markdown, text, diff, and status primitives without introducing a common trait unless substitution is required.
- [ ] 6.4 Create transcript, composer, status, Preview placeholder, and Input Page Regions that bind view models without performing I/O or mutating domain text during render.
- [ ] 6.5 Create main and Preview Pane modules and a Screen/layout module, initially rendering the current single-column composition only.
- [ ] 6.6 Move overlays and central cursor placement to the Screen while retaining the current focus and hidden hardware-cursor behavior.
- [ ] 6.7 Add architecture checks enforcing `Screen -> Pane -> Region -> Component` and rejecting upward imports.
- [ ] 6.8 Make the main-pane characterization tests pass for effective widths, including line counts, colors, backgrounds, spacing, content, copy provenance, and cursor behavior.
- [ ] 6.9 Re-run snapshot and continuous-frame benchmarks and correct any material regression introduced solely by rendering reorganization.

## 7. Finalize responsive and Preview foundations

- [ ] 7.1 Use the characterization suite to determine and document the minimum usable main-pane width while preserving composer, status, and transcript behavior.
- [ ] 7.2 Audit existing keybindings and select a conflict-free full-screen Preview toggle for the narrow fallback before implementing the two-pane layout.
- [ ] 7.3 Define `PreviewPolicy`, `PreviewTarget`, `PreviewRef`, `PreviewContent`, `PreviewState`, request identifiers, keys, revisions, and `PreviewPaneState` in `e-tui`.
- [ ] 7.4 Implement shared Preview cache keys and invalidation independent of transcript structural cache invalidation.
- [ ] 7.5 Extend normalized tool/timeline annotations to provide inline Link, Diff, Lines, SearchResult, Command, Path, Markdown, PlainText, and complete-source fallback previews.
- [ ] 7.6 Add target reconciliation that resets scroll on target change and preserves target identity through streaming, settlement, and history prepend.

## 8. Add the normal-mode Preview sidebar

- [ ] 8.1 Implement wide layout rectangles using the 60-percent/configured main width rule and a minimum 32-column full-height Preview pane.
- [ ] 8.2 Design and implement the Preview Region's empty, ready, loading, and error presentation by reusing current Components where suitable and adding sidebar-specific themed Components only where needed.
- [ ] 8.3 Ensure every bundled theme supplies defaults for any new sidebar token and verify that no Preview token changes main-pane surface styling.
- [ ] 8.4 Implement `FollowLatestBlock`, including same-target refresh for streaming and tool settlement and no target change on history prepend.
- [ ] 8.5 Render specialized inline Preview content and complete-source fallback with independent Preview scroll and visible-row materialization.
- [ ] 8.6 Implement the narrow main-only fallback and the selected full-screen Preview toggle without overloading Reading View navigation keys.
- [ ] 8.7 Add TestBackend tests for wide/narrow rectangles, main-pane visual continuity, Preview states/content/colors, target changes, scroll reset, and transcript rewrap provenance.
- [ ] 8.8 Extend frame diagnostics and benchmarks to report Preview rebuild/patch work and verify target changes do not rebuild the transcript.

## 9. Build the semantic Reading Document and layout

- [ ] 9.1 Define stable `BlockId`, `ItemId`, `ReadingBlock`, `ReadingItem`, copy payload, Block taxonomy, and width-independent `ReadingDocument` values.
- [ ] 9.2 Derive paragraph, code, list-row, Mermaid, table/custom, user text, and visible reasoning Blocks from existing Markdown/display identities and provenance.
- [ ] 9.3 Derive one stable Block per normalized tool lifecycle and attach adapter-provided file/link/other Items without parsing raw tool JSON in `e-tui`.
- [ ] 9.4 Preserve Block and Item identities across resize, pane-width changes, streaming growth, tool settlement, animation, theme rematerialization, and history prepend.
- [ ] 9.5 Reconcile semantic removal/insertion through canonical surface replacement and exclude hidden reasoning and zero-row fallback content.
- [ ] 9.6 Build `ReadingLayout` Block row ranges, gutter rails, and wrapped Item fragments from the existing width-aware transcript layout rather than a second wrapper.
- [ ] 9.7 Add characterization tests for all Block kinds, Markdown links, file Items, wrapped fragments, stable IDs, surface replacement, hidden reasoning, and history prepend.
- [ ] 9.8 Keep old Copy Mode operational over the shared provenance/layout while the new semantic model is validated.

## 10. Add Reading View Block mode

- [ ] 10.1 Add `ReadingViewState` with exactly one Block cursor, optional Item cursor, retained anchors, and invariants enforced by update methods.
- [ ] 10.2 Route the selected Reading View binding (`Ctrl+V` candidate until the compatibility gate) through central input precedence, reject entry when blocked or empty, and select the eligible Block nearest viewport center.
- [ ] 10.3 Implement Block-mode `j`/Down, `k`/Up, `l`/Right, `y`, and `Esc` behavior with clamped boundaries.
- [ ] 10.4 Implement top-third/bottom-third page scrolling and semantic cursor restoration after resize or rewrap.
- [ ] 10.5 Render Night current-Block backgrounds and Bark gutter rails without changing content x-position, wrapping, or explicit inline backgrounds.
- [ ] 10.6 Preserve and restore composer buffer, cursor, multiline, and completion state across Reading View entry and exit.
- [ ] 10.7 Switch Preview policy to the Reading Block on entry, keep it stable under live appends, and restore latest-Block policy on exit.
- [ ] 10.8 Add TestBackend and transition tests for entry selection, navigation, rail geometry, backgrounds, draft restoration, resize, live updates, copying, and blocking-input precedence.

## 11. Add Item mode and spatial navigation

- [ ] 11.1 Order Items visually from their shared-layout fragments and enter Item mode on the first or horizontally anchored Item.
- [ ] 11.2 Implement left/right spatial ranking by primary-axis distance, secondary-axis distance, and visual/document-order tie breaks.
- [ ] 11.3 Implement up/down navigation within a Block and cross-Block behavior with retained horizontal position and Block-mode fallback when the adjacent Block has no Items.
- [ ] 11.4 Implement left-boundary and `Esc` exits to Block mode while preserving the Block cursor.
- [ ] 11.5 Render one local Item highlight over all wrapped fragments without overwriting explicit local span backgrounds.
- [ ] 11.6 Give Item Preview precedence over Block Preview while keeping `y` bound to the complete owning Block.
- [ ] 11.7 Add exhaustive directional-boundary, wrapped/multiline Item, ownership-invariant, highlight, copy, and Preview-precedence tests.

## 12. Add deferred Preview resolution

- [ ] 12.1 Implement `ResolvePreview` execution ports and bounded resolver tasks in `e-dsh` for file, diff, and adapter/kernel-backed data.
- [ ] 12.2 Return Preview completion events with request ID, key, revision, and bounded success/error content through the existing event channel.
- [ ] 12.3 Implement cache reuse and stale-result protection so late values may populate cache but cannot replace a newer visible target.
- [ ] 12.4 Connect loading/error rendering and one scheduled draw request to deferred target selection and completion.
- [ ] 12.5 Add deterministic race tests for A/B ordering, revision changes, cache revisit, errors, mode switching, and normal/Reading cache sharing.
- [ ] 12.6 Add lock-discipline tests proving no resolver or effect executor awaits while holding a `TuiApp` guard.

## 13. Replace row-oriented Copy Mode

- [ ] 13.1 Verify `Ctrl+V` delivery and bracketed paste behavior in Windows Terminal, ConHost, and each supported Linux terminal.
- [ ] 13.2 If any supported terminal fails the compatibility gate, select, document, implement, and retest an alternate Reading View binding before proceeding.
- [ ] 13.3 Verify Reading View parity for Markdown, code, table, Mermaid, activity, reasoning, wrapped content, history, resize, and complete-source clipboard payloads.
- [ ] 13.4 Remove `Ctrl+B`, row cursor/range selection, selection anchors, Copy Mode overlays, range navigation, and caches used only by row selection.
- [ ] 13.5 Rename surviving copy-specific provenance/layout types to semantic reading or provenance names where their responsibility has changed.
- [ ] 13.6 Update help overlay, key references, notices, and UI regression tests in the same change that removes Copy Mode.
- [ ] 13.7 Add source guards confirming old Copy Mode state and rendering branches are absent while atomic provenance remains.

## 14. Final cleanup, documentation, and gates

- [ ] 14.1 Remove temporary forwarding facades and adapters whose callers have migrated, while retaining one canonical transcript and config schema/default source.
- [ ] 14.2 Decide whether the main loop should own `TuiApp` directly; remove `Arc<Mutex<_>>` only if scoped tests show the simplification is low risk.
- [ ] 14.3 Run the architecture suite across both crates and verify no SCC, no `e-tui -> e-dsh`, no DSH imports in `e-tui`, downward rendering imports, one transcript path, and event-returning Preview tasks.
- [ ] 14.4 Run `cargo fmt --all`, workspace Clippy for all targets/features required by the project, scoped/full Rust tests appropriate for this large refactor, and all UI regression tests.
- [ ] 14.5 Run bridge tests and `node tools/sync-protocol-contract.mjs --check` if any bridge or wire shape changed; otherwise record that the internal normalized contract required no protocol bump.
- [ ] 14.6 Re-run release startup, snapshot, continuous streaming/scrolling/animation, Preview, Reading navigation, and responsive-layout benchmarks and resolve any redline failure.
- [ ] 14.7 Update `AGENTS.md`, `docs/client.md`, `docs/design.md`, `docs/README.md`, help text, config/theme references, and `README.md` only where user-facing install or keybinding basics materially changed.
- [ ] 14.8 Mark completed phases in `docs/plan/` or archive the plan after current architecture documentation has become authoritative.
