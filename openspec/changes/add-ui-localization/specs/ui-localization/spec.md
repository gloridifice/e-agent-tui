## ADDED Requirements

### Requirement: Frontend UI text resolves from embedded locale catalogs
All frontend-owned user-visible UI text SHALL be looked up from compile-time embedded locale catalogs at an explicit locale derived from the active `Config.language`. The lookup helpers SHALL be stateless: they SHALL NOT read or mutate process-global locale state, and the client SHALL NOT call `rust_i18n::set_locale`. The embedded catalogs SHALL contain at least `en` (default and fallback) and `zh-CN`, and missing keys SHALL fall back to `en` and then to the key itself.

#### Scenario: Default configuration renders English
- **WHEN** the client runs with the embedded default configuration
- **THEN** every frontend-owned UI surface renders its text from the `en` locale catalog

#### Scenario: Chinese configuration renders localized chrome
- **WHEN** `Config.language` is `zh-CN`
- **THEN** frontend-owned UI chrome (settings pages, Input Pages, overlays, status bar, notices, command descriptions, lifecycle projections, preview states, and the `dshe` shutdown message) renders the `zh-CN` catalog text

#### Scenario: Missing translation falls back
- **WHEN** a key exists in `en` but is absent from `zh-CN`
- **THEN** the `en` text is rendered rather than an empty string

#### Scenario: User content is never translated
- **WHEN** transcript messages, tool output, session titles, or host-provided strings are rendered
- **THEN** they appear verbatim regardless of the active language

### Requirement: Locale catalogs keep key parity across languages
The embedded locale files SHALL be validated by an automated test asserting that every key present in the `en` catalog is also present in the `zh-CN` catalog, so neither locale silently regresses when keys are added.

#### Scenario: A key is added only to English
- **WHEN** a developer adds a key to `locales/en.yml` without adding it to `locales/zh-CN.yml`
- **THEN** the locale parity test fails

### Requirement: Language is a validated persisted config value
`Config` SHALL persist a `language` field whose accepted values are exactly `en` and `zh-CN`, validated during strict deserialization. The embedded default TOML SHALL set `language = "en"`. Existing user config files that omit the field SHALL inherit the default through the known-key overlay, and an invalid value SHALL be rejected through the existing safe-fallback path.

#### Scenario: Old config inherits the language default
- **WHEN** a user config file predating the `language` field is loaded
- **THEN** the effective `language` is `en`

#### Scenario: User sets Chinese
- **WHEN** a user config sets `language = "zh-CN"`
- **THEN** the value round-trips through load/save and all UI text resolves at `zh-CN`

#### Scenario: Unknown locale rejected
- **WHEN** a user config sets `language = "fr"`
- **THEN** strict deserialization rejects the value and the loader takes the existing documented fallback path

### Requirement: Language switching applies immediately through the config pipeline
Changing the language in `/settings` SHALL flow through the existing `ConfigChanged` pipeline: the new value is written to the live config, render caches whose rows embed localized text (transcript render cache and markdown layout registry) are invalidated, the config is persisted immediately, and the next rendered frame shows the new language without restart. `/reload` SHALL re-read `language` from disk through the same reload path.

#### Scenario: Switch language in settings
- **WHEN** the user confirms the language row's other option in `/settings`
- **THEN** the configuration is persisted, transcript/markdown caches are invalidated, and the next frame renders the settings page, status bar, and chrome in the new language

#### Scenario: Reload picks up an edited file
- **WHEN** the user edits `language` in the config file on disk and runs `/reload`
- **THEN** the reloaded configuration's language takes effect for all subsequently rendered UI text

#### Scenario: Already-admitted transcript blocks stay in their original language
- **WHEN** the language changes after lifecycle or notice blocks were admitted to the transcript
- **THEN** previously admitted blocks keep their original text while new chrome renders in the new language

### Requirement: Locale resolution stays out of geometry caches
The locale used for row text assembly SHALL NOT become part of any width/theme-keyed layout cache key, and localized text SHALL continue to flow through the existing width-aware wrapping, truncation, and display-column rules.

#### Scenario: CJK chrome text wraps by display width
- **WHEN** a localized `zh-CN` chrome string exceeds its row width
- **THEN** it is wrapped or truncated by the existing display-width rules without splitting grapheme clusters

#### Scenario: Language switch does not rebuild geometry
- **WHEN** the language changes and caches are invalidated
- **THEN** rebuilt rows still derive their geometry from the existing width/theme keys, with no locale-specific cache dimension

### Requirement: Test locale is explicit and deterministic
Tests SHALL obtain localized output by constructing `Config` with an explicit `language` value or passing an explicit locale string to the lookup helpers; no test SHALL depend on or mutate a process-global locale.

#### Scenario: Parallel tests in different languages
- **WHEN** one test renders with `en` and another with `zh-CN` concurrently
- **THEN** both assertions observe their own configured language
