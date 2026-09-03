## ADDED Requirements

### Requirement: Settings page text is locale-resolved with value-driven choices
The settings Input Page category names, row labels, descriptions, and choice labels SHALL resolve from the active language at render time. Settings metadata and choice handling SHALL use stable translation keys and locale-independent values rather than rendered labels. Value-returning helpers SHALL return values such as `left`, `compact`, or `en`, and translation SHALL occur only for presentation.

#### Scenario: Settings renders in English
- **WHEN** the settings Input Page is rendered with `Config.language = "en"`
- **THEN** its categories, labels, descriptions, and choice labels display English catalog text

#### Scenario: Settings renders in Chinese
- **WHEN** the settings Input Page is rendered with `Config.language = "zh-CN"`
- **THEN** its categories, labels, descriptions, and choice labels display Simplified Chinese catalog text

#### Scenario: Choice round-trip survives translation
- **WHEN** the user edits a boolean, alignment, thinking-display, or language row in either language
- **THEN** the confirmed locale-independent option maps to the same underlying config value

### Requirement: Language row is available in the Behavior category
The settings Input Page SHALL include a Language row in the Behavior category with exactly the supported `en` and `zh-CN` values. Confirming a value SHALL use the existing settings change effect, SHALL keep the page open, and SHALL preserve its active category and logical focused row while text is re-rendered.

#### Scenario: Switch language and keep editing
- **WHEN** the user confirms the other option on the Language row
- **THEN** language is applied and persisted immediately, the Behavior category remains active, and focus remains on the Language row

### Requirement: Every Input Page localizes frontend-owned chrome
Settings, login, model, effort, theme, resume, and question Input Pages SHALL resolve their frontend-owned headers, loading/empty/unavailable states, owned option labels, markers, field labels, action labels, and key-hint footers from the active language. Their shared shell geometry, navigation, effects, and transport behavior SHALL remain unchanged. Provider/session/question content and adapter error bodies SHALL remain verbatim.

#### Scenario: Localized loading and empty states
- **WHEN** a model, effort, resume, login, or theme page displays a frontend-owned loading or empty state
- **THEN** that state is rendered in the active language

#### Scenario: Localized footer preserves controls
- **WHEN** any Input Page renders its key-hint footer in either language
- **THEN** the displayed text is localized while the documented keys perform the same actions

#### Scenario: External page content remains unchanged
- **WHEN** an Input Page combines localized chrome with provider, model, session, question, or error content
- **THEN** only the frontend-owned chrome is translated

### Requirement: Input Page focus identity is locale-independent
Input Page focus reconciliation SHALL identify targets by stable logical identity such as setting keys, provider/model ids, session ids, question ids, and option indices, never by localized display text. A language change SHALL not alter focus nodes, edit state, selection state, or the page effect that confirmation produces.

#### Scenario: Settings focus survives a language switch
- **WHEN** language changes while the Language setting row is focused
- **THEN** the same setting key remains focused after labels are re-rendered

#### Scenario: Dynamic roster focus survives localization
- **WHEN** a localized roster page refresh still contains the focused provider, model, session, question, or option identity
- **THEN** the same logical target remains focused regardless of rendered language
