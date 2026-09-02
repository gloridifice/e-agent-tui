## 1. i18n foundation

- [ ] 1.1 Add `rust-i18n = "4"` to `crates/e-tui/Cargo.toml`; commit updated root `Cargo.lock`; confirm no `load-path` feature
- [ ] 1.2 Create `crates/e-tui/locales/en.yml` and `zh-CN.yml` (`_version: 1`, namespaced flat keys) with an initial subset (categories, language row, common states)
- [ ] 1.3 Create `crates/e-tui/src/i18n.rs`: single `i18n!("locales", fallback = "en")` call, `tr(locale, key)` explicit-locale lookup, interpolation helper over `replace_patterns`, `Language` enum (`en`/`zh-CN`) with `FromStr`/`Display`/serde; export from `lib.rs`
- [ ] 1.4 Add locale parity unit test asserting every `en` key exists in `zh-CN` (via the generated backend); add a grep-style guard test or review step that `set_locale` is never called in `e-tui`

## 2. Config + pipeline

- [ ] 2.1 Add `language` to `Config` (validated `Language`, strict deserialize rejects unknown values) and `language = "en"` to `assets/default_config.toml` behavior section; config round-trip/invalid/inherit tests
- [ ] 2.2 Sync derived locale into render paths: add `locale` to `RenderOptions`; add `language` sync in the `ConfigChanged` controller branch and `apply_reloaded_config` (mirror `paste_placeholder_chars`), keeping `markdown_layout.invalidate_all()` + `transcript_cache.invalidate()`
- [ ] 2.3 Regression tests: language switch through settings invalidates caches and the next frame renders the new language (TestBackend); `/reload` re-reads `language`

## 3. Settings page restructure

- [ ] 3.1 Convert `CATEGORIES` and `ItemDef` label/desc to i18n keys; localize the settings renderer (`ui/pages/settings.rs`) via the active locale
- [ ] 3.2 Make choices value-driven: `ItemKind::Choice` options become `(value, key)` pairs; `apply` matches values; `page_align_label`/`thinking_display_label`/`bool_str` become value-returning; update all affected settings tests to locate rows by key and assert values
- [ ] 3.3 Add the language row (`settings.language.*`) to the Behavior category with `en`/`zh-CN` options wired to `Config.language`; test confirm-switch persistence and that the page stays open

## 4. Input Pages localization

- [ ] 4.1 Localize login page (headers, menu, provider states, proxy form/delete, footers) — `ui/pages/login.rs`
- [ ] 4.2 Localize model, effort, and theme pages (headers, loading/empty/unavailable states, footers, default markers) — `ui/pages/{model,effort,theme}.rs`
- [ ] 4.3 Localize resume and question pages (headers, loading, no-match, footers, next/submit actions) — `ui/pages/{resume,question}.rs`
- [ ] 4.4 Verify focus identities remain locale-independent (settings focus ids by category+key; roster pages by provider/model ids) and add/adjust tests for focus surviving a language switch

## 5. Overlays, help, notices

- [ ] 5.1 Localize the help overlay (`ui/overlay.rs`) — all 13 rows incl. title — threading `locale` through `help_overlay`
- [ ] 5.2 Localize `/help` markdown (`help.rs`) — section headings and the interaction-help body per locale
- [ ] 5.3 Localize suggestion popup, approval card, queue strip, and toast accessory chrome (`ui/accessories.rs`), adding the `locale` parameter; update accessory tests
- [ ] 5.4 Localize clipboard notice formatting (`notice.rs` `show_clipboard`) and its test

## 6. Transcript chrome + preview states

- [ ] 6.1 Localize status bar and title row (`ui/status.rs`): `^h Help`, `新会话`/`新对话` placeholders; update status/title tests to English defaults plus a zh-CN case
- [ ] 6.2 Localize Thinking indicator label (`Thinking...`) at creation sites in `runtime/state.rs` (label resolved from the active locale at admission time) and keep display/copy semantics
- [ ] 6.3 Localize history-load hint rows (`ui/transcript.rs`) and the `提示词注入` label (label constant resolved per frame from locale; docs note the locale-resolved wording)
- [ ] 6.4 Localize code/table/mermaid chrome in `render.rs` (`· N 行`, collapse hints `… 收起 N 行 [Enter 展开]`, mermaid failure note) via `RenderOptions.locale`; update render tests asserting English chrome
- [ ] 6.5 Localize Preview empty/loading/error states (`ui/region/preview.rs`: `No preview`, `• Loading preview…`, `Preview error:`)

## 7. Command catalog + controller notices + lifecycle

- [ ] 7.1 Convert `BUILTIN_COMMANDS` descriptions/hints to i18n keys; resolve them in `match_command_catalog` (locale parameter) and `/new` mode fillers where owned; keep command names untouched
- [ ] 7.2 Thread the active locale into suggestion descriptions (`InputState` language sync) and update input/command-catalog tests
- [ ] 7.3 Localize controller/command notices and errors (`设置保存失败`, `正在创建新对话`, `用法: /...`, `没有可阅读的内容`, `此命令不接受图片`, `已重载配置…`, clipboard errors) — `runtime/{controller,command}.rs`
- [ ] 7.4 Localize lifecycle projections (`（已中断）`, `（已阻塞）`, `（会话异常中断）`, `（达到模型输出 token 上限）`, history-truncation notice) at projection time — `projection/lifecycle.rs`, `runtime/state.rs`; keep already-admitted blocks untranslated

## 8. e-dsh message, docs, final sweep

- [ ] 8.1 Replace `DSH_SERVER_CLOSED_MESSAGE` in `crates/e-dsh/src/main.rs` with a key resolved through `e_tui::i18n` at the current config locale
- [ ] 8.2 Update `docs/subsystem/client/architecture.md`: locale-resolved labels it cites verbatim (`新会话`, `提示词注入`), the `language` config field, and the localization invariant (explicit-locale lookup, cache invalidation on switch, no `set_locale`)
- [ ] 8.3 Final sweep: `rg '[\p{Han}]'` over production code shows no remaining user-visible Chinese literals (test fixtures and intentionally Chinese data excluded); run scoped `cargo test` for touched modules plus full `cargo fmt --all` and `cargo clippy --all-targets`
