## Context

The Rust workspace (`e-dsh` / `e-pi` / `e-tui`) localizes nothing today: `e-tui` carries ~250 hard-coded Chinese UI strings plus a few English ones, `e-dsh` has one shutdown message, `e-pi` has none. All config flows through a single persisted `Config` schema (embedded `default_config.toml` + known-key overlay + strict deserialize), and `/settings` edits flow through `PageEffect::ConfigChanged` → controller (cache invalidation + `UiAction::PersistConfig`) → adapter persistence. Theme switching already exercises this entire pipeline.

Key constraints from the architecture docs and AGENTS.md:

- `e-tui` performs no filesystem I/O; rendering is cache-heavy (transcript render cache, markdown layout registry, Preview styled-layout cache) and language changes must invalidate every layer that embeds locale-dependent text.
- Test discipline: deterministic tests, no parallel-test flakiness; UI changes need TestBackend regression tests.
- `t!` expands against `crate::_rust_i18n_try_translate`, so only the crate that invokes `i18n!` may use `t!`; `i18n!("locales")` paths are resolved relative to that crate's directory.
- `rust_i18n::set_locale` is process-global (arc-swap), shared across all test threads.

## Goals / Non-Goals

**Goals:**

- English (default) and Simplified Chinese UI, chosen per process via persisted config, switchable live in `/settings` with immediate apply + save.
- Compile-time embedded translations; zero runtime locale file I/O; no new runtime dependencies in the shipped binary beyond `rust-i18n`'s support layer.
- All frontend-owned user-visible strings localized (settings pages, Input Pages, overlays, transcript chrome, notices, command catalog, lifecycle projections, `dshe` shutdown message).
- Deterministic testing under the existing parallel test harness.

**Non-Goals:**

- Runtime user-supplied locale catalogs (`load-path` feature is off).
- Translating user content, tool output, host/DSH strings, or adapter-originated error bodies (e.g. Preview file-limit errors stay English).
- Re-translating already-admitted transcript blocks when the language changes.
- Locale detection from `LANG`/environment (explicit config only).
- Bridge/Node.js-side localization.

## Decisions

### D1. rust-i18n with explicit-locale lookup, not global `set_locale`

**Decision:** `e_tui::i18n` wraps the single `i18n!("locales", fallback = "en")` call and exposes `tr(locale, key)` (and an interpolation helper) that pass `locale = ...` into each lookup. The locale originates from `Config.language` wherever a `config`/`state` handle exists.

**Alternatives considered:**
- Global `set_locale` at startup + bare `t!` calls — fewer call-site changes, but the process-global locale makes parallel `cargo test` tests interfere (a zh-CN test flips every concurrent en assertion), and render-path locale becomes invisible/implicit. Rejected for test determinism and explicitness.
- Hand-rolled static match tables — no dependency, but we lose catalogs, interpolation, parity tooling, and the crate's maintained fallback chain for free.

**Consequence:** accessory renderers that lacked a config handle (`help_overlay`, suggest popup, approval card, queue strip, toast) gain a `locale: &str` parameter; `RenderOptions` gains a `locale` field so `render.rs` chrome rows resolve it.

### D2. `Config.language` as a validated transparent enum

**Decision:** `Language` enum (`En`, `ZhCn`) with `FromStr`/`Display`/serde as `"en"`/`"zh-CN"`, following the existing `HexRgb`/`RevealRate` validated-transparent-value pattern. `default_config.toml` gains `language = "en"` in the behavior section, so old user files inherit `en` via the existing known-key overlay, and `/settings` persists the same key.

**Alternatives considered:** raw `String` — rejected: strict deserialization must reject unknown locales (consistent with `deny_unknown_fields` strictness and validated-value discipline).

### D3. Language switch rides the existing config pipeline

**Decision:** the settings row's `apply` sets `config.language`; the existing `PageEffect::ConfigChanged` handler in the controller performs `state.config = ui.config`, `markdown_layout.invalidate_all()`, `transcript_cache.invalidate()`, syncs derived input state, and pushes `UiAction::PersistConfig`. `/reload` uses `apply_reloaded_config`. No new effect types, no new wire messages.

**Rationale:** theme switching proves this path; caches must be invalidated because cached rows embed localized chrome text (`Thinking...`/`· N 行`/collapse hints), and markdown layouts embed localized collapse hints. The only addition: sync `ui.input.language` (mirroring `paste_placeholder_chars`) for command-catalog description localization.

**Alternatives considered:** forcing a restart on language change — simpler but hostile to the "即改即存" settings contract.

### D4. Settings metadata becomes key-driven with value-driven choices

**Decision:** `ItemDef.label`/`desc` hold i18n keys; `CATEGORIES` becomes key-based; `ItemKind::Choice` options become `(value, key)` pairs where the stored value is locale-independent (`"en"`, `"center"`, `"compact"`...), while display labels are translated at render time. Label-returning helpers (`page_align_label`, `thinking_display_label`) become value-returning (`"center"`, `"compact"`), and `apply` matches on values instead of Chinese display strings (`v == "开"`).

**Rationale:** the current Chinese-label round-trips break the moment labels are translated. Value-driven choices keep settings behavior locale-independent while the presentation layer translates.

**Alternatives considered:** keeping display-label matching and translating only when locale is zh — rejected: en labels would still need separate matching arms, doubling the round-trip bug surface.

### D5. Locale catalogs: v1 split files, namespaced keys, parity test

**Decision:** `crates/e-tui/locales/en.yml` + `zh-CN.yml` (`_version: 1`), flat dotted keys namespaced by surface (`settings.*`, `status.*`, `notice.*`, `render.*`, `command.*`, `overlay.*`, `lifecycle.*`, `preview.*`, `input_page.*`). Interpolation uses `%{name}`. A unit test iterates the generated backend's `messages_for_locale` to assert every `en` key exists in `zh-CN`.

**Alternatives considered:** `_version: 2` single-file-per-module layout — fine but no advantage at this size; the split-file layout matches the two-locale scope.

### D6. Migration batches keep each step compilable and testable

**Decision:** implement in five independently verifiable batches: (1) i18n module + config + settings page restructure, (2) overlays/help/notice/accessories, (3) transcript chrome + preview states, (4) command catalog + controller/command/lifecycle notices, (5) `e-dsh` message + docs. Each batch migrates whole call sites (no mixed-language pages), updates affected tests, and passes `cargo test -p e-tui` scoped runs.

### D7. Test strategy: English-default assertions, explicit zh-CN configs

**Decision:** default `Config` assertions become English; zh-CN coverage constructs `Config::default()` with `language = zh` (a small test helper). Settings tests locate rows by stable i18n keys instead of display labels (more robust than either language's label). Existing CJK width/wrapping tests keep their literals — they test user-content handling, not UI chrome.

## Risks / Trade-offs

- [Parallel-test contamination via global state] → the design never calls `set_locale`; `tr` is explicit-locale and stateless. CI grep for `set_locale(` in `e-tui` guards this.
- [Missed strings leave mixed-language screens] → locale parity test for key completeness; batch discipline migrates whole files; `rg '[\p{Han}]'` sweep at the end of each batch over production code keeps drift visible.
- [Cache not invalidated on language switch leaves stale chrome] → language rides the same invalidation set as theme; add a UI-layer regression test asserting cached rows re-render in the new language after `ConfigChanged`.
- [~250-key migration churn touches render/cache-sensitive code] → translation happens at string-assembly boundaries only; no layout, width, or cache-key logic changes except the added invalidation already covered by theme switching. `RenderOptions.locale` must not enter existing geometry caches that are keyed by width/theme only — it is used for row text assembly, same layer as theme.
- [Existing Chinese users flip to English after upgrade] → documented behavioral note; one `/settings` visit restores zh-CN. Accepted: default-English is the stated requirement.
- [rust-i18n proc-macro adds compile time] → one extra proc-macro crate; acceptable relative to syntect/wasmi already present. RustSec advisory only affects `rust-i18n-support 3.0.0`; v4 is unaffected.

## Migration Plan

1. Land `e_tui::i18n` + `Config.language` + settings restructure with both locale files populated for the migrated subset; old configs inherit `en`.
2. Migrate remaining surfaces batch-by-batch (each batch green independently).
3. Final sweep: production-code Han-literal audit (test fixtures excepted), `cargo fmt --all`, `cargo clippy --all-targets`, scoped test runs, root `Cargo.lock` committed, docs updated.
4. Rollback: revert the change; `language` key in user configs becomes an ignored unknown key under the existing overlay policy — no persistent damage.
