## Context

The shared Rust frontend now lives in `e-tui`. `TuiApp` owns the canonical `Config`, `CatalogModel`, `InteractionModel`, transcript/render state, and Preview state; each executable composition root owns filesystem effects and temporarily moves `InteractionModel` out while the shared controller or renderer uses it. Runtime work is split across `runtime/controller/{agent,effect,input,terminal}.rs` and `runtime/state/{mod,reduction,session}.rs`, while rendering follows `Screen -> Pane -> Region -> Component`.

The UI currently contains both hard-coded Chinese and hard-coded English chrome. Locale-dependent text is produced in three forms:

- frame-resolved chrome such as pages, overlays, status, composer placeholders, resize placeholders, transcript hints, and Preview labels;
- cached presentation such as Markdown collapse rows, transcript rows, Preview styled layouts, and suggestion descriptions;
- admitted semantic text such as Thinking labels, lifecycle blocks, local `/help` Markdown, controller errors, notices, and deferred-new status text.

`e-tui` must remain free of filesystem I/O and provider wire types. `command_catalog` must also remain a dependency-free top-level leaf under the architecture test. Tests run in parallel, so `rust_i18n::set_locale` cannot be used safely.

## Goals / Non-Goals

**Goals:**

- Provide a complete English-default and Simplified Chinese UI for both `dshe` and `pie`.
- Resolve every frontend-owned string with an explicit typed language derived from `Config.language`.
- Apply and persist language changes immediately without a restart.
- Preserve the current frontend/runtime boundaries, lock discipline, cache semantics, focus identities, and provider-neutral data model.
- Keep tests deterministic under parallel execution.

**Non-Goals:**

- Runtime or user-supplied locale catalogs, environment-based locale detection, or bridge-side localization.
- Translating user/session/question/approval content, host command descriptions, preset/model/effort names, tool names/output, paths, identifiers, or adapter-provided error bodies.
- Translating operational CLI/setup/launcher diagnostics outside the interactive frontend, except the existing managed-DSH shutdown confirmation.
- Re-translating text already admitted into transcript or transient semantic state.
- Adding locale to wire messages or protocol contracts.

## Decisions

### D1. One explicit-language `rust-i18n` boundary

`crates/e-tui/src/i18n.rs` will contain the only `i18n!("locales", fallback = "en")` invocation and expose lookup helpers that accept `Language` explicitly, including interpolation arguments. Callers will not invoke `rust_i18n::set_locale` or read its process-global locale.

`Language` will be a small `Copy` enum serialized as exactly `"en"` or `"zh-CN"`. `Config` depends on this value type; the i18n module does not depend on `Config`, which avoids a cycle and keeps lookup usable at narrow boundaries.

A global locale was rejected because concurrent English and Chinese tests would race. Hand-written match tables were rejected because catalogs, interpolation, fallback, and parity checks are better handled by the localization crate.

### D2. English remains the only default source

`Config.language` will be added to the directly deserializable schema, while `language = "en"` is added to `assets/default_config.toml`. The embedded TOML remains the sole source of persisted defaults. The existing known-key overlay lets old files inherit English, and the strict deserialize path rejects unsupported locale values.

A raw `String` was rejected because unsupported locales must fail through the same validated-value path as other constrained config fields.

### D3. Locale flows through existing owners, with only deliberate derived state

Frame rendering reads `state.config.language` and passes it down the existing layered render calls. `RenderOptions` carries `Language` for Markdown-owned chrome. Reducers and controllers obtain the language from `RuntimeState.config` when they admit localized semantic text.

`InputState` will retain a derived `Language` copy because it owns stateful suggestion descriptions and composer display projection (`[Image …]` and `[N text pasted]`), just as it already retains derived paste/history config. `ConfigChanged` and reload update this copy and rebuild an open suggestion from the sole `CatalogModel`. `PromptInput` remains locale-neutral; queue rendering formats image placeholders with the active language.

No provider-neutral event/request, catalog payload, or semantic Preview content type gains provider- or process-global locale state.

### D4. Language changes reuse controller config effects and explicitly invalidate localized caches

The settings row continues to emit `PageEffect::ConfigChanged`. Both `runtime/controller/input.rs` and `runtime/controller/effect.rs` will synchronize the new config into `TuiApp`, update derived `InputState` language, and preserve `UiAction::PersistConfig` behavior.

The update invalidates:

- `RenderState.markdown_layout`, because code/table/Mermaid collapse text is materialized there;
- `RenderState.transcript_cache`, because transcript rows include localized Thinking and chrome text;
- `PreviewPaneState`'s styled-layout cache, because structured Preview rows include localized fallback, search, metrics, and line-anchor labels.

An open suggestion is rebuilt in the new language. Locale is not added to existing width/theme/cache identity structs; explicit invalidation is the transition mechanism, and rebuilt geometry still uses the existing width/theme/source rules. Preview semantic cache entries and reveal frontiers remain intact.

Restart-on-change was rejected because settings already promise immediate apply/save. Adding locale to every cache key was rejected because each process has one active configured language and the current cache owners already expose explicit invalidation.

### D5. Metadata remains stable and locale-neutral

`settings::CATEGORIES` and `ItemDef` label/description fields become translation keys. Choice options become `(value, label_key)` pairs: persisted values such as `"left"`, `"compact"`, `"en"`, and boolean states never depend on translated labels. Settings focus ids use the stable item key, not rendered text.

`command_catalog` remains dependency-free. `BuiltinCommand` stores description/hint keys, and command matching returns enough metadata for `input.rs` and `help.rs` to resolve built-in text. Integrated command descriptions and hints remain host-provided verbatim. This preserves the architecture assertion that `command_catalog` is a leaf.

Status effort formatting follows the same rule: `CatalogModel` exposes locale-neutral effort data, while `ui/status.rs` localizes only the frontend-owned `Effort:` prefix and `Default` fallback. Provider-supplied effort labels remain unchanged.

### D6. Frame chrome and admitted text use different timing

Pure chrome is translated when rendered, so a language switch changes it on the next frame. This includes Input Pages, overlays, composer and queue placeholders, accessories, resize placeholders, status, history/context labels, Markdown chrome, and Preview chrome.

Frontend semantic strings are translated when admitted. Existing transcript blocks and in-flight notices keep their original text; only newly admitted Thinking/lifecycle/help/error/notice/draft text uses the new language. User and provider text is always copied verbatim. Adapter errors may receive a localized frontend prefix, but their body is not translated.

This avoids mutating transcript history or semantic provider content when language changes.

### D7. Catalogs are namespaced and parity-tested

The catalogs use `_version: 1` and surface namespaces such as `settings.*`, `input_page.*`, `overlay.*`, `composer.*`, `status.*`, `render.*`, `preview.*`, `command.*`, `runtime.*`, and `lifecycle.*`. Interpolation uses named arguments.

A parity test asserts every English key exists in `zh-CN`. Lookup tests cover English fallback and unknown-key fallback. A source guard rejects `set_locale(` in `e-tui` production code. The final literal audit classifies remaining Han text rather than blindly removing fixtures: user/host data tests and non-user-visible comments may remain.

### D8. Managed-DSH shutdown uses the final effective language

`crates/e-dsh/src/main.rs` currently prints the shutdown confirmation in `run_tui`, outside the inner `run` function that owns the mutable config. The inner run path will return or otherwise expose the last effective `Language` before teardown so the outer launcher-release path can translate the confirmation after alternate-screen restoration. If startup fails before a config is established, English is used.

The Pi adapter needs no corresponding executable message. No adapter imports locale catalogs directly; `e-dsh` calls the public `e_tui::i18n` helper.

## Risks / Trade-offs

- **Missed strings produce mixed-language screens** -> Migrate complete surfaces, add English/Chinese TestBackend cases, and perform categorized Han and visible-English literal audits.
- **A switch leaves stale cached text** -> Invalidate Markdown, transcript, and Preview styled layouts and rebuild open suggestions in both config-change paths; add regression tests for all four.
- **Localization breaks architecture layering** -> Keep `command_catalog`, provider DTOs, and semantic payloads locale-neutral; pass `Language` downward or resolve at callers; run the architecture test.
- **Localized placeholders change cursor/display width** -> Continue deriving cursor mapping and row counts from the same localized `InputDisplay`, using existing grapheme/display-width helpers.
- **Provider text is accidentally translated** -> Catalog only fixed frontend chrome and interpolate external values verbatim; test mixed localized chrome with unchanged host content.
- **Previously Chinese users see English after upgrade** -> This is the intentional default; old configs inherit `en`, and `/settings` can persist `zh-CN` immediately.
- **`rust-i18n` adds compile time and API coupling** -> Isolate all macro use in one module and expose a small project-owned helper API.

## Migration Plan

1. Add catalogs, typed language lookup, config support, and deterministic catalog tests.
2. Add settings metadata/value changes and controller synchronization, including complete cache invalidation and open-suggestion refresh.
3. Migrate frame-resolved surfaces in coherent batches, then admitted runtime/projection text.
4. Localize the managed-DSH shutdown confirmation and update current architecture documentation.
5. Run scoped localization/config/settings/input/controller/render/Preview/projection tests, the architecture gate, `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --all-targets`, and OpenSpec validation.

Rollback is a normal code revert. A persisted `language` key left in a user file is ignored by the pre-change known-key overlay, so rollback does not corrupt other settings.

## Open Questions

None.
