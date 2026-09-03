## ADDED Requirements

### Requirement: Frontend UI text resolves from embedded locale catalogs
All frontend-owned user-visible text SHALL be looked up from compile-time embedded locale catalogs using an explicit `Language` derived from the active `Config.language`. Lookup SHALL be stateless and SHALL NOT read or mutate process-global locale state. The catalogs SHALL include `en` as the default and fallback and `zh-CN` as the Simplified Chinese locale; lookup SHALL fall back to English and then the key when a translation is unavailable.

Frontend-owned text includes Input Page and settings chrome, help, suggestions, composer and queue placeholders, accessories, resize placeholders, status labels, transcript chrome, Preview chrome, local command/controller messages, lifecycle text, and the managed-DSH shutdown confirmation.

#### Scenario: Default configuration renders English
- **WHEN** either executable renders the shared frontend with the embedded default configuration
- **THEN** frontend-owned text renders from the `en` catalog

#### Scenario: Chinese configuration renders localized chrome
- **WHEN** `Config.language` is `zh-CN`
- **THEN** every frontend-owned surface renders its corresponding `zh-CN` text

#### Scenario: Missing translation falls back
- **WHEN** a requested key is unavailable in `zh-CN` but exists in `en`
- **THEN** the English text is returned rather than an empty string

#### Scenario: Managed DSH closes after early startup failure
- **WHEN** a managed DSH service is released after startup failed before an effective config language was available
- **THEN** the post-terminal shutdown confirmation uses English fallback text

### Requirement: External and semantic content remains verbatim
Localization SHALL NOT translate user input, transcript content, session titles, question or approval content, host command descriptions, preset/provider/model/effort names, tool names or output, paths, identifiers, or adapter-provided error bodies. A frontend-owned prefix or surrounding label MAY be localized while the embedded external value remains unchanged.

#### Scenario: Localized prefix contains an adapter error
- **WHEN** the active language is `zh-CN` and the frontend displays an adapter error with a frontend-owned prefix
- **THEN** the prefix is localized and the adapter error body is preserved byte-for-byte

#### Scenario: Host catalog content is displayed
- **WHEN** a suggestion or Input Page displays host-provided names, descriptions, or hints
- **THEN** those values are shown verbatim in either language

#### Scenario: User content is rendered
- **WHEN** transcript, queue, or Preview content contains user or tool text
- **THEN** the content is unchanged regardless of the active language

### Requirement: Locale catalogs keep key parity across languages
The embedded locale files SHALL be validated by an automated test asserting that every key present in the English catalog is also present in the Simplified Chinese catalog.

#### Scenario: A key is added only to English
- **WHEN** a developer adds an English key without adding the same key to `zh-CN`
- **THEN** the locale parity test fails

### Requirement: Language is a validated persisted config value
`Config` SHALL persist a `language` field whose accepted values are exactly `en` and `zh-CN`, validated during strict deserialization. The embedded default TOML SHALL set `language = "en"`. Existing user config files that omit the field SHALL inherit the default through the known-key overlay, and an invalid value SHALL use the existing safe-fallback path.

#### Scenario: Old config inherits the language default
- **WHEN** a valid user config predating the `language` field is loaded
- **THEN** the effective language is `en` while its other overrides remain effective

#### Scenario: User sets Chinese
- **WHEN** a user config sets `language = "zh-CN"`
- **THEN** the value round-trips through load and save and subsequent frontend text resolves with `zh-CN`

#### Scenario: Unknown locale is rejected
- **WHEN** a user config sets `language` to any other value
- **THEN** strict deserialization rejects it and the loader takes the existing safe-fallback path

### Requirement: Language switching applies immediately through the existing config pipeline
Changing language in `/settings` SHALL use the existing `PageEffect::ConfigChanged` pipeline, and `/reload` SHALL apply a language read from disk through the existing reload path. Both paths SHALL update the live `TuiApp.config`, synchronize locale-dependent derived interaction state, preserve immediate persistence behavior, and request rendering without a process restart.

#### Scenario: Switch language in settings
- **WHEN** the user confirms the other language in `/settings`
- **THEN** the setting is persisted, the settings page stays open, and the next frame renders current chrome in the new language

#### Scenario: Reload picks up an edited language
- **WHEN** the user edits `language` in the config file and runs `/reload`
- **THEN** all subsequently resolved frontend text uses the reloaded language

#### Scenario: Open suggestion is refreshed
- **WHEN** locale-dependent suggestion state is present while a config language change is applied
- **THEN** its frontend-owned descriptions are rebuilt in the new language without changing the query, selected logical command, or host-provided descriptions

### Requirement: Language switching invalidates every locale-dependent presentation cache
A language change SHALL explicitly invalidate the Markdown layout registry, transcript render cache, and Preview styled-layout cache while preserving semantic Preview cache entries and reveal state. The switch SHALL NOT add locale to existing width/theme/source cache identity structs; rebuilt rows SHALL continue to use the existing geometry and display-width rules.

#### Scenario: Cached transcript and Markdown chrome changes language
- **WHEN** localized transcript or Markdown chrome has been cached and the language changes
- **THEN** the next frame rematerializes that chrome in the new language

#### Scenario: Cached structured Preview changes language
- **WHEN** a structured Preview layout containing localized labels is cached and the language changes
- **THEN** its styled layout is rebuilt in the new language without discarding the semantic Preview value or resetting its reveal frontier

#### Scenario: Locale is not a cache dimension
- **WHEN** localized rows are rebuilt after a language change
- **THEN** geometry is recomputed using the existing width, theme, source, and revision rules without adding language to persistent cache keys

#### Scenario: CJK chrome obeys display-width rules
- **WHEN** Simplified Chinese chrome exceeds its available row width
- **THEN** existing grapheme-safe wrapping or truncation rules are applied

### Requirement: Stateful localized text has defined admission semantics
Pure frame chrome SHALL resolve in the active language on each render. Frontend-owned text stored as transcript or transient semantic state SHALL resolve using the active language when admitted and SHALL not be rewritten solely because the language later changes.

#### Scenario: Existing local transcript block survives a switch
- **WHEN** a Thinking, lifecycle, controller-error, or local `/help` block was admitted before the language changed
- **THEN** that block keeps its original text while newly admitted blocks use the new language

#### Scenario: Composer placeholder changes on the next frame
- **WHEN** an existing image or atomic-paste block is visible and language changes
- **THEN** its display placeholder is rebuilt in the new language without changing the underlying prompt, cursor boundary, or attachment bytes

### Requirement: Test locale is explicit and deterministic
Tests SHALL obtain localized output by constructing config or derived state with an explicit `Language`, or by passing `Language` directly to project-owned lookup helpers. Production code and tests SHALL NOT call `rust_i18n::set_locale`.

#### Scenario: Parallel tests use different languages
- **WHEN** English and Simplified Chinese render tests execute concurrently
- **THEN** each test observes only its explicitly selected language

#### Scenario: Global locale mutation is introduced
- **WHEN** production `e-tui` source contains a call to `set_locale`
- **THEN** the localization guard test fails
