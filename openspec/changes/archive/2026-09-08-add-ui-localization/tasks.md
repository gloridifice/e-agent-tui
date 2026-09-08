## 1. Localization foundation

- [x] 1.1 Add `rust-i18n = "4"` to `crates/e-tui/Cargo.toml`, update the root `Cargo.lock`, and verify runtime locale loading features are not enabled
- [x] 1.2 Create `crates/e-tui/locales/en.yml` and `zh-CN.yml` with `_version: 1`, namespaced keys, English fallback text, and the initial config/settings/common strings
- [x] 1.3 Add `crates/e-tui/src/i18n.rs` as the sole `i18n!` invocation, with a `Language` enum serialized as `en`/`zh-CN` plus explicit-language lookup and named-interpolation helpers; export the module from `lib.rs`
- [x] 1.4 Add tests for enum parse/serde behavior, English and unknown-key fallback, English/`zh-CN` key parity, and a source guard rejecting `set_locale(` in production `e-tui`

## 2. Config propagation and cache invalidation

- [x] 2.1 Add `Config.language: Language` and `language = "en"` to `assets/default_config.toml`; test direct defaults, old-file inheritance, valid round-trip, unknown-value rejection, and safe fallback
- [x] 2.2 Add derived `Language` to `InputState` and `RenderOptions`, and pass the active language from `TuiApp.config` through `presentation.rs`, transcript materialization, Preview weak-Markdown rendering, and other explicit render-option construction sites
- [x] 2.3 Update `runtime/controller/input.rs` `ConfigChanged` handling and `runtime/controller/effect.rs` reload handling to synchronize `TuiApp.config` and `InputState.language`, rebuild an open suggestion from `CatalogModel`, invalidate Markdown/transcript caches, invalidate only the Preview styled-layout cache, and preserve the existing owned persistence effect
- [x] 2.4 Add controller regressions proving settings and reload language changes update the next frame, refresh open built-in suggestion text, invalidate all locale-dependent presentation caches, and preserve Preview semantic cache/reveal state

## 3. Settings metadata and page

- [x] 3.1 Convert `settings.rs` category, item label, and description metadata to stable locale keys while keeping the module independent of `input_page`
- [x] 3.2 Convert every settings choice to locale-independent `(value, label_key)` options; replace Chinese-label comparisons and label-returning config helpers with stable values, including boolean, alignment, and thinking-display choices
- [x] 3.3 Add the Language row to the Behavior category with `en` and `zh-CN` values and wire confirmation to the unchanged `PageEffect::ConfigChanged` behavior
- [x] 3.4 Localize `ui/pages/settings.rs`, change settings focus ids in `input_page.rs` to stable item keys, and test English default, explicit Chinese, immediate apply/save, page retention, and focus retention

## 4. Input Pages

- [x] 4.1 Localize login headers, menu descriptions, provider state, proxy fields/actions/delete confirmation, loading/error surroundings, and footers in `ui/pages/login.rs`, preserving provider/proxy/error values verbatim
- [x] 4.2 Localize model, effort, and theme page headers, default/current markers, loading/empty/unavailable states, and footers in `ui/pages/{model,effort,theme}.rs`, preserving catalog names and ids
- [x] 4.3 Localize resume and question page headers, title placeholders, loading/no-match states, next/submit/select labels, and footers in `ui/pages/{resume,question}.rs`, preserving session/question content
- [x] 4.4 Add Input Page TestBackend cases for both languages and focus-reconciliation tests proving provider/model/session/question/settings identities and active edits do not depend on translated labels

## 5. Frame chrome, composer, help, and accessories

- [x] 5.1 Localize `ui/overlay.rs` help rows and `help.rs` local Markdown, passing explicit language into `/help`; preserve integrated command descriptions and already-admitted help blocks
- [x] 5.2 Localize suggestion popup headers/footer, approval labels, Goal/Plan/Todo labels, queue overflow text, and clipboard toast text in `ui/accessories.rs`, `ui.rs`, and `notice.rs`
- [x] 5.3 Localize composer image and atomic-paste placeholders in `input.rs`/`ui/region/composer.rs`, plus queued `PromptInput` image placeholders in `action.rs`/`ui/accessories.rs`; test cursor mapping, atomic boundaries, filenames, and user text remain unchanged in both languages
- [x] 5.4 Localize message/Preview pane labels shown by the separator-drag placeholder in `ui/screen.rs` without changing drag geometry or percentages
- [x] 5.5 Update opaque-popup, toast, approval/queue, composer-width, and resize-placeholder TestBackend regressions to use English defaults and add focused `zh-CN` cases

## 6. Status, transcript, Markdown, and Preview

- [x] 6.1 Localize `^h Help`, new-session/new-conversation placeholders, and effort prefix/default text in `ui/status.rs`; refactor `catalog.rs` to expose locale-neutral effort status data and preserve provider effort labels
- [x] 6.2 Localize the Thinking label and deferred-new/history-truncation text at admission in `runtime/state/{mod,reduction,session}.rs`, preserving the rule that previously admitted text is not rewritten
- [x] 6.3 Localize history-load hints and the injected-context label in `ui/transcript.rs`, resolving them per frame through the active language and retaining existing width/copy semantics
- [x] 6.4 Localize code/table/Mermaid line counts, collapse prompts, and Mermaid failure chrome in `render.rs` via `RenderOptions.language`; update `transcript_layout.rs` tests for explicit invalidation without adding language to entry identity
- [x] 6.5 Localize Preview empty/loading/error text and structured Preview chrome in `ui/region/preview.rs`, including Link/Search fallbacks, `at`, line-count/duration metrics, and mutation line anchors, while preserving paths, queries, commands, output, and error bodies
- [x] 6.6 Add English and Chinese rendering regressions for status, transcript chrome, Markdown collapse rows, structured Preview rows, and Preview styled-layout invalidation with semantic cache/reveal preservation

## 7. Command, controller, and lifecycle admission

- [x] 7.1 Convert `BUILTIN_COMMANDS` descriptions and input hints to locale keys without importing i18n into `command_catalog`; return locale-neutral built-in metadata from matching and keep integrated command text verbatim
- [x] 7.2 Resolve built-in command text in `input.rs` and `help.rs`, update suggestion/catalog refresh paths in `runtime/controller/agent.rs`, and cover English/Chinese built-ins mixed with unchanged host commands
- [x] 7.3 Localize usage/model-resolution and deferred-new messages in `runtime/command.rs`, passing explicit language through `LocalCommandContext` and interpolating command/reference values verbatim
- [x] 7.4 Localize clipboard/config/runtime-error wrappers and reading/image-command notices in `runtime/controller/{agent,effect,input,terminal}.rs`, preserving external error bodies and current lock/effect discipline
- [x] 7.5 Localize fixed lifecycle outcomes in `projection/lifecycle.rs` and history admission in `runtime/state`, passing the active language at projection/admission time and testing that old blocks remain unchanged after a switch

## 8. Adapter message, documentation, and validation

- [x] 8.1 Update `crates/e-dsh/src/main.rs` so the outer launcher-release path resolves the managed-service shutdown confirmation with the final effective `Language`, falling back to English if startup ended before config initialization; update its focused tests
- [x] 8.2 Update `docs/subsystem/client/architecture.md` for explicit-language lookup, `Config.language`, locale-neutral command/settings metadata, locale-dependent cache invalidation, and locale-resolved labels; add a concise English/Chinese availability note to `README.md`
- [x] 8.3 Audit production literals with categorized Han and visible-English searches, excluding comments, protocol/CLI diagnostics, test fixtures, user/host data, identifiers, and intentionally untranslated external content; add any missed frontend-owned keys to both catalogs
- [x] 8.4 Run scoped config, i18n, settings, input, Input Page, controller, lifecycle, render, transcript, Preview, and `e-dsh` shutdown tests plus the client architecture gate
- [x] 8.5 Run `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --all-targets`, `openspec validate add-ui-localization --strict`, and confirm `openspec status --change add-ui-localization` remains apply-ready
