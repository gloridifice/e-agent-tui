## ADDED Requirements

### Requirement: Language is a persisted validated config field
The persisted `Config` schema SHALL declare a `language` field exactly once with accepted values `en` and `zh-CN`, validated during the single strict deserialization. The embedded default TOML SHALL remain the sole default source (`language = "en"`), and existing user files that omit the key SHALL inherit the default through the existing known-key overlay.

#### Scenario: Field added to schema and defaults
- **WHEN** a developer adds `language` to `Config` and the embedded default TOML
- **THEN** no second persisted structure or per-field apply list is needed for it to load, overlay, and persist

#### Scenario: Old user file inherits the default
- **WHEN** a valid user config omits `language`
- **THEN** loading succeeds with `language = "en"` and other overrides are preserved

#### Scenario: Invalid locale value takes the safe fallback
- **WHEN** a user file sets `language` to any value other than `en` or `zh-CN`
- **THEN** the strict known-value error path applies and the client falls back safely to the embedded defaults

### Requirement: Language is live-editable with immediate save and apply
The `/settings` Input Page SHALL expose a language row (in the Behavior category) whose confirmed value is validated, saved to the client config, and applied immediately using the existing config-update behavior, including render-cache invalidation and immediate persistence.

#### Scenario: Confirm a language change
- **WHEN** the user confirms the other language option on the language row
- **THEN** the value is validated, persisted, and applied immediately so the next frame renders in the new language

#### Scenario: Reload re-reads the language
- **WHEN** the user runs `/reload` after editing `language` on disk
- **THEN** the reloaded configuration resolves the new `language` value during loading without disk reads during rendering
