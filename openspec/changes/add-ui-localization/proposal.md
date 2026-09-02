## Why

The frontend UI is hard-coded in Chinese, with a handful of hard-coded English strings; there is no way for an English-speaking user to use the client. We adopt `rust-i18n` to localize all frontend-owned UI text to English (default) and Simplified Chinese, and expose a language switch in `/settings`.

## What Changes

- Add the `rust-i18n` crate (v4) to `e-tui` with compile-time embedded locale files (`crates/e-tui/locales/en.yml`, `locales/zh-CN.yml`, fallback `en`).
- Add a new `e_tui::i18n` module as the single `i18n!` call site with an explicit-locale lookup helper (`tr(locale, key)` / interpolation helper); rendering paths resolve the locale from config instead of process-global state.
- Add a validated `language` field to `Config` (values `en` | `zh-CN`, default `en`) in the embedded default TOML; old user configs inherit the default through the existing known-key overlay.
- Add a 语言 / Language choice row to the `/settings` 行为 (Behavior) category. Confirming it flows through the existing `PageEffect::ConfigChanged` pipeline: caches invalidated, config persisted immediately, next frame renders in the new language. `/reload` follows the same path.
- Restructure the settings `ItemDef` metadata so labels/descriptions/option lists are locale keys with value-driven choices (replacing Chinese-label value round-trips like `v == "开"`), enabling full settings-page localization.
- Migrate all frontend-owned user-visible strings (~250 keys) to translation keys across: settings page and all Input Pages, help overlay and `/help` markdown, suggestion popup, approval card, queue strip, toast, status bar (`^h Help`, `新会话`/`新对话`), Thinking indicator, history-load hints, code/table collapse hints, `提示词注入` label, Preview empty/loading/error states, command catalog descriptions, controller notices/errors, lifecycle projections, and the `dshe` shutdown message.
- Existing UI-layer tests that assert Chinese rendering move to English-default assertions; new zh-CN assertions construct `Config { language: zh }` explicitly. Add a locale parity test asserting every `en` key exists in `zh-CN`.
- Scope boundaries: adapter-originated error bodies (e.g. Preview file-limit errors) stay English; user content, tool output, and already-admitted transcript blocks are never translated; no `load-path` runtime-locale feature.

## Capabilities

### New Capabilities
- `ui-localization`: frontend-owned UI text is resolved from compile-time embedded locale catalogs keyed by an explicit locale derived from `Config.language`, with English as the default and fallback language, and a `/settings` language switch that applies and persists immediately through the existing config pipeline.

### Modified Capabilities
- `declarative-config-overlay`: a new validated `language` field joins the persisted `Config` schema, sourced from the embedded default TOML, inherited by old user files through the known-key overlay, and live-editable via `/settings` with immediate save/apply.
- `input-page`: the settings Input Page's labels, descriptions, and choice options become locale-resolved (value-driven choices instead of Chinese-label round-trips), and a language row joins the Behavior category.

## Impact

- **Dependencies**: `crates/e-tui/Cargo.toml` gains `rust-i18n = "4"` (proc-macro + support; YAML parsing stays compile-time only). Root `Cargo.lock` updated.
- **Code**: `crates/e-tui/src/i18n.rs` (new), `config.rs` (`language` field + validation), `settings.rs` (key/value-driven restructure), `command_catalog.rs`, `help.rs`, `notice.rs`, `runtime/{controller,command,state}.rs`, `projection/lifecycle.rs`, `render.rs` (`RenderOptions` gains locale), `ui/{status,overlay,accessories,transcript}.rs`, `ui/region/preview.rs`, `ui/pages/*`, `input.rs` (description localization state), `assets/default_config.toml`; `crates/e-dsh/src/main.rs` (shutdown message via `e_tui::i18n`).
- **Assets**: new `crates/e-tui/locales/` directory with `en.yml` and `zh-CN.yml`.
- **Tests**: existing Chinese-rendering assertions migrate to English defaults; zh-CN coverage via explicit `Config`; new locale parity, config round-trip, and cache-invalidation-on-language-switch regression tests.
- **Behavioral note**: existing users with pre-`language` configs get English until they switch once in `/settings` (the default for the new field is `en`).
- **Docs**: `docs/subsystem/client/architecture.md` updates for the locale-resolved labels it cites verbatim (`新会话`, `提示词注入`) and the new `language` config field.
