## Why

The shared `e-tui` frontend still mixes hard-coded Chinese and English text, so neither `dshe` nor `pie` can present a consistent English interface. The recent client refactor moved the runtime, interaction state, and layered renderer into `e-tui`, so the localization plan must now follow those ownership boundaries and invalidate every locale-dependent presentation cache.

## What Changes

- Add `rust-i18n` v4 to `e-tui` with compile-time embedded `en` and `zh-CN` catalogs; English is the default and fallback.
- Add a single `e_tui::i18n` boundary that performs stateless, explicit-language lookup and interpolation without process-global locale state.
- Add a validated persisted `Config.language` value (`en` or `zh-CN`, default `en`) and expose it as a Language row in the `/settings` Behavior category.
- Apply a settings or `/reload` language change through the existing `PageEffect::ConfigChanged` and config-reload controller paths, synchronizing derived interaction state and invalidating transcript Markdown/layout, transcript render, and Preview styled-layout caches before the next frame.
- Convert settings metadata and built-in command metadata to stable translation keys and locale-independent values. Keep `command_catalog` dependency-free and resolve its built-in text at its callers so the current architecture gate remains valid.
- Localize all frontend-owned visible chrome in `e-tui`, including Input Pages, help, suggestions, composer image/paste placeholders, accessories, resize placeholders, status and effort labels, transcript chrome, Preview chrome, command/controller messages, and lifecycle text. Host/provider content, user content, tool output, identifiers, and error bodies remain verbatim.
- Localize the post-TUI managed-DSH shutdown confirmation using the final effective language, with English fallback if startup failed before config became available.
- Update affected tests to assert English defaults and explicit `zh-CN` rendering, add locale-key parity and no-global-locale guards, and update the current client architecture documentation.

## Capabilities

### New Capabilities
- `ui-localization`: Compile-time English and Simplified Chinese catalogs, explicit-language frontend rendering, immediate language switching, cache invalidation, and deterministic locale tests.

### Modified Capabilities
- `declarative-config-overlay`: Add `language` to the single validated persisted `Config` schema and its immediate save/reload behavior.
- `input-page`: Localize settings and all other Input Pages while preserving locale-independent values, focus identities, navigation, and effects.

## Impact

- **Dependency/assets**: `crates/e-tui/Cargo.toml`, root `Cargo.lock`, and new `crates/e-tui/locales/{en,zh-CN}.yml`.
- **Frontend state/config**: `config.rs`, `i18n.rs`, `input.rs`, `settings.rs`, `catalog.rs`, `command_catalog.rs`, `help.rs`, `notice.rs`, `presentation.rs`, and `lib.rs`.
- **Runtime/reduction**: `runtime/command.rs`, `runtime/controller/{agent,effect,input,terminal}.rs`, `runtime/state/{mod,reduction,session}.rs`, and locale-aware calls into the shared renderer.
- **Rendering**: `render.rs`, `transcript_layout.rs`, `ui.rs`, `ui/screen.rs`, `ui/accessories.rs`, `ui/overlay.rs`, `ui/status.rs`, `ui/transcript.rs`, `ui/region/{composer,preview}.rs`, and `ui/pages/*.rs`.
- **Adapter executable**: `crates/e-dsh/src/main.rs` only for the managed-service shutdown confirmation; no wire/protocol or bridge change.
- **Documentation/tests**: current client architecture plus scoped config, settings, input, controller, projection, rendering, Preview-cache, and `e-dsh` shutdown tests.
