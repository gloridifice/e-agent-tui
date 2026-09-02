## ADDED Requirements

### Requirement: Settings page text is locale-resolved with value-driven choices
The settings Input Page's category names, row labels, descriptions, and choice option labels SHALL resolve from the active locale at render time. Choice rows SHALL match and persist locale-independent values rather than display labels, so choice round-trips (`get`/`apply`) remain correct under any language. Value-returning helpers SHALL return locale-independent values (e.g. `"center"`, `"compact"`, `"en"`) and translation SHALL happen only at presentation.

#### Scenario: Settings renders in English
- **WHEN** the settings Input Page is rendered with `Config.language = "en"`
- **THEN** categories, labels, descriptions, and choice labels display English text

#### Scenario: Settings renders in Chinese
- **WHEN** the settings Input Page is rendered with `Config.language = "zh-CN"`
- **THEN** categories, labels, descriptions, and choice labels display the `zh-CN` catalog text

#### Scenario: Choice round-trip survives translation
- **WHEN** the user edits a boolean, alignment, thinking-display, or language row while the UI is displayed in either language
- **THEN** the confirmed option maps to the same underlying config value it would map to in the other language

### Requirement: Language row in the settings Behavior category
The settings Input Page SHALL include a language row in the Behavior category offering exactly the supported locales (`en`, `zh-CN`), showing the current value, and switching language on confirmation through the existing confirm-edit interaction without closing the page or discarding other settings state.

#### Scenario: Switch language and keep editing
- **WHEN** the user confirms the other language on the language row
- **THEN** the language is applied and persisted immediately, the settings page remains open, and subsequent rows render in the new language

### Requirement: Other Input Pages render localized text
Login, model, effort, theme, resume, and question pages SHALL resolve their headers, loading/empty/unavailable states, option labels they own, and key-hint footers from the active locale, using the shared Input Page shell without changing focus, navigation, or wire behavior.

#### Scenario: Localized loading and empty states
- **WHEN** a model or effort page is open while its catalog has not arrived
- **THEN** the loading state text renders in the active language

#### Scenario: Localized footers
- **WHEN** any Input Page renders its key-hint footer
- **THEN** the hint text renders in the active language

### Requirement: Focus identities stay locale-independent
Input Page focus reconciliation SHALL keep identifying targets by stable logical identity (ids, provider/model names, category/value identifiers), never by localized display labels, across language switches and refreshes.

#### Scenario: Focus survives a language switch
- **WHEN** the language changes while a settings page is open
- **THEN** the focused row remains the same logical item
