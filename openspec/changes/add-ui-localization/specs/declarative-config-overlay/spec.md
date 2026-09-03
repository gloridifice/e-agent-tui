## ADDED Requirements

### Requirement: Language is a persisted validated config field
The persisted `Config` schema SHALL declare `language` exactly once with accepted serialized values `en` and `zh-CN`. The embedded default TOML SHALL remain the sole default source and SHALL define `language = "en"`. Existing user files that omit the key SHALL inherit it through the existing recursive known-key overlay.

#### Scenario: Field is added to the canonical schema
- **WHEN** `language` is present in `Config` and the embedded default TOML
- **THEN** it loads, overlays, and persists without a second persisted structure or per-field copy list

#### Scenario: Old user file inherits the default
- **WHEN** a valid user config omits `language`
- **THEN** loading succeeds with `language = "en"` and preserves its other valid overrides

#### Scenario: Invalid locale takes the safe fallback
- **WHEN** a user file sets `language` to a value other than `en` or `zh-CN`
- **THEN** strict deserialization fails and the existing safe config fallback behavior applies

### Requirement: Language is live-editable with immediate save and apply
The `/settings` Input Page SHALL expose `language` as a live-editable setting. A confirmed value SHALL update the executable-owned config value and canonical `TuiApp.config`, synchronize locale-dependent derived interaction state, invalidate locale-dependent presentation caches, and produce the existing owned `UiAction::PersistConfig` effect. Rendering SHALL perform no config or catalog filesystem I/O.

#### Scenario: Confirm a language change
- **WHEN** the user confirms the other supported language in `/settings`
- **THEN** the value is applied to live state, locale-dependent caches are invalidated, an owned config snapshot is persisted, and the next frame uses the new language

#### Scenario: Reload re-reads language
- **WHEN** the user runs `/reload` after editing `language` on disk
- **THEN** the adapter-owned loader returns the updated canonical `Config` and the shared controller applies it without rendering-time disk reads
