> **状态：已完成。** 目前由 `page_core`、`input_page` 与 `RuntimeController` 维护同一页面生命周期；
> 不要恢复 page-specific main-loop branches 或 overlay model/theme renderer。

## 1. Shared Input Page foundation

- [x] 1.1 Add the Input Page module structure, closed `InputPage` enum, unified session owner, page outcome/effect types, and exports from `client/src/lib.rs`.
- [x] 1.2 Implement stable focus IDs, explicit directional focus graphs, arrow/`hjkl` key normalization, disabled-target skipping, and focus reconciliation after target refresh.
- [x] 1.3 Implement the shared text/secret editor and browse-versus-edit event routing so printable `hjkl` is retained during editing.
- [x] 1.4 Implement shared viewport/ensure-visible helpers and unit tests for focus movement, disappeared targets, editor confirmation/cancellation, and bounded scrolling.
- [x] 1.5 Implement the borderless Input Page shell and reusable header, footer, status, focus-row, choice, text-field, and masking render primitives with exact 2-column/1-row padding.

## 2. Settings page migration

- [x] 2.1 Adapt `SettingsState` to the unified page contract with stable category and item focus IDs, focusable category tabs, and non-actionable read-only rows.
- [x] 2.2 Migrate settings text and choice editing to the shared editor/focus behavior while preserving validation, immediate save, and immediate runtime application effects.
- [x] 2.3 Rebuild settings rendering from shared Input Page primitives, remove duplicate row indentation, and retain wrapped descriptions and focused-item scrolling.
- [x] 2.4 Update settings unit/UI tests for tab activation, single focus, arrow/`hjkl` equivalence, choice/text editing, read-only skipping, and exact padding.

## 3. Login page migration

- [x] 3.1 Adapt login menu, provider, account, proxy-list, and proxy-form states to stable actionable focus targets and shared editor behavior.
- [x] 3.2 Prevent non-writable credentials and informational/loading rows from receiving actionable focus while preserving API-key masking and existing bridge actions.
- [x] 3.3 Add a proxy deletion confirmation subpage whose Cancel/Esc path sends nothing and whose explicit Delete action sends the existing `LoginProxyDelete` message.
- [x] 3.4 Rebuild login rendering with shared shell/status/row/editor primitives and preserve in-page loading, Codex progress, and bridge error presentation.
- [x] 3.5 Add login state and TestBackend regressions for dynamic roster reconciliation, text-mode `hjkl`, non-writable providers, proxy delete confirmation, masking, and uniform padding.

## 4. Model and theme page migration

- [x] 4.1 Move model picker state out of overlay rendering into a `ModelPage` with loading state, stable provider/model IDs, one two-column focus graph, and current-selection markers independent of focus.
- [x] 4.2 Implement model catalog refresh reconciliation, provider activation, empty-provider behavior, and model activation returning `ModelSet` plus page close.
- [x] 4.3 Replace the bordered model overlay with a bounded borderless Input Page renderer using shared shell, focus, viewport, empty, loading, and footer primitives.
- [x] 4.4 Move theme picker state into a `ThemePage` whose theme rows are focusable, swatches are decorative, and activation updates config, emits the persistence effect, and closes the page.
- [x] 4.5 Replace the bordered theme overlay with a bounded borderless Input Page renderer and add model/theme state and UI tests for navigation, activation, loading/empty data, stable refresh focus, and exact padding.

## 5. Main-loop and command integration

- [x] 5.1 Replace the separate settings, login, model-picker, and theme-picker options in `main.rs` and `runtime_command.rs` with one mutually exclusive Input Page owner.
- [x] 5.2 Replace the four page-specific key branches with unified outcome processing that applies config changes and bridge sends only after page borrows and mutex guards have ended.
- [x] 5.3 Route login/model server frames through the matching active Input Page, preserve global model status updates, and safely ignore late page-specific responses.
- [x] 5.4 Update the main renderer arguments so Input Pages use the bottom replacement chunk while session/help/copy overlays remain independent, then remove obsolete model/theme overlay rendering paths.
- [x] 5.5 Update all renderer call sites, examples, and tests for the revised render context and verify closing a page restores the unchanged ordinary input buffer and transcript viewport.

## 6. Cross-page regression coverage and documentation

- [x] 6.1 Add shared TestBackend coverage proving all four pages replace the input area, draw no border/floating `Clear`, retain transcript/status/title layout, and use exactly 2-column/1-row inner padding.
- [x] 6.2 Add narrow/short-terminal regressions proving each page clips or scrolls without panic, out-of-range focus, or overlap into status/title rows.
- [x] 6.3 Update the TUI help overlay and any relevant input tests for the common Input Page arrow/`hjkl`, Enter, and Esc behavior.
- [x] 6.4 Update `README.md`, `AGENTS.md`, and `docs/design.md` to describe the Input Page abstraction, migrated `/settings` `/login` `/model` `/theme` behavior, focusable settings tabs, and confirmed proxy deletion.

## 7. Validation

- [x] 7.1 Run `cargo fmt --all -- --check` and resolve formatting issues.
- [x] 7.2 Run `cargo clippy --workspace --all-targets` and resolve new warnings without weakening the existing significant-drop lint discipline.
- [x] 7.3 Run the complete `cargo test` suite, rerun any known flaky tool-card test once if necessary, and record that no bridge/protocol change was required.

Validation record: formatting and Clippy completed successfully; after review fixes, `cargo test` passed 224 tests (220 library + 4 binary). This client-only change required no bridge or protocol-contract update.
