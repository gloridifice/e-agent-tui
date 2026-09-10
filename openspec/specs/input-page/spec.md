# input-page Specification

## Purpose
Provide a shared, focus-driven input-area interface for browsing settings, credentials, models, and themes while preserving the ordinary composer draft.

## Requirements

### Requirement: Unified Input Page lifecycle
The client SHALL represent `/settings`, `/login`, `/model`, and `/theme` as mutually exclusive Input Pages, with at most one Input Page active at a time. Opening one of these commands SHALL replace the ordinary input area without discarding its text or transcript state.

#### Scenario: Open a configuration command
- **WHEN** the user executes `/settings`, `/login`, `/model`, or `/theme`
- **THEN** the corresponding Input Page becomes the sole active configuration page and replaces the ordinary input area

#### Scenario: Close a page
- **WHEN** the active Input Page is closed
- **THEN** the ordinary input area returns with its prior buffer and the transcript and scroll state unchanged

### Requirement: Shared ruled replacement layout
Every Input Page SHALL render in the bottom page area used in place of the input bar, without a floating window, `Clear`, or background fill. A shared full-width ruled shell SHALL place a prompt-style command header between horizontal rules; page-internal dividers SHALL use the Umber semantic tone.

#### Scenario: Render a page at normal terminal size
- **WHEN** any Input Page is rendered
- **THEN** the terminal background remains visible, full-width rules frame the prompt-style header and page, and no floating or centered overlay is drawn

#### Scenario: Render page interaction states
- **WHEN** an actionable item is focused or a value is selected
- **THEN** focused text uses the Sage semantic tone, selected text uses the Coral semantic tone, and neither state adds a background fill, except Resume title/date columns retain their fixed tones with focus conveyed by a separate marker

#### Scenario: Render on a small terminal
- **WHEN** an Input Page is rendered in a terminal too small for all body rows
- **THEN** layout calculations remain bounded, visible content is clipped or scrolled, and rendering does not panic or overlap the status and title rows

### Requirement: Single actionable focus
When an Input Page has at least one enabled actionable element, it SHALL expose exactly one visibly focused actionable element. When no enabled actionable element exists, it SHALL expose no focus target. Configurable page directional actions, defaulting to arrows and h/j/k/l, SHALL move focus through enabled actionable elements according to the page focus graph, and confirm, defaulting to Enter, SHALL execute only the focused element. Read-only, informational, unavailable, and disabled elements SHALL not receive actionable focus.

#### Scenario: Navigate with equivalent keys
- **WHEN** the user presses either configured binding for a browse direction
- **THEN** focus moves to the same neighboring actionable element for both key forms

#### Scenario: Activate focused element
- **WHEN** the user invokes confirm in browse mode
- **THEN** only the currently focused actionable element is executed

#### Scenario: Skip unavailable elements
- **WHEN** navigation encounters a read-only, disabled, loading, or informational element
- **THEN** focus skips that element or remains on the nearest enabled actionable element

### Requirement: Stable focus across dynamic updates
Input Pages backed by asynchronous data SHALL identify focus targets by stable logical identity and SHALL retain focus when the same target remains available after a refresh. If the target disappears, the page SHALL move focus to a valid nearby fallback or to no target when none exist.

#### Scenario: Model catalog refresh retains target
- **WHEN** a model catalog update still contains the currently focused provider or model ID
- **THEN** the same logical element remains focused regardless of list reordering

#### Scenario: Focused target disappears
- **WHEN** a login or model refresh removes the focused logical element
- **THEN** focus is reconciled to a valid actionable fallback without using an out-of-range index

### Requirement: Browse and text-edit modes
The Input Page system SHALL distinguish browse, choice-edit, and text-edit contexts. In browse mode configured navigation SHALL move focus; in text-edit mode printable characters including h/j/k/l/q SHALL be inserted into the editor. Configurable edit confirm and cancel SHALL default to Enter and Esc and SHALL not pass to ordinary input or enclosing page navigation. Resume filtering and free-text questions SHALL not inherit browse letter navigation.

#### Scenario: Type Vim navigation letters
- **WHEN** a text or secret editor is active and the user types hjklq
- **THEN** those five characters are added to the editor and page focus does not move

#### Scenario: Cancel an edit
- **WHEN** the user invokes edit cancel while an editor is active
- **THEN** the unconfirmed value is discarded and the page remains open at the edited element

### Requirement: Shared page status presentation
Input Pages SHALL provide a consistent header, body, footer, loading, error, and key-hint presentation inside the shared padded area. Errors specific to login/configuration actions SHALL remain in the page rather than being projected into the transcript unless persistence itself fails under existing client policy.

#### Scenario: Await asynchronous data
- **WHEN** a login or model page is open and its requested catalog has not arrived
- **THEN** the page shows a loading state rather than reporting an empty catalog as a completed result

#### Scenario: Display login write failure
- **WHEN** the bridge returns a login error for the active login page
- **THEN** the error is shown in the Input Page status area and does not become a transcript message

### Requirement: Settings page interaction
The settings Input Page SHALL expose category tabs as actionable focus targets and editable setting rows as actionable targets. Enter on a category SHALL activate it; Enter on an editable row SHALL start its text or choice editor; read-only rows SHALL not be actionable. Confirmed changes SHALL retain the existing immediate save and immediate apply behavior.

#### Scenario: Activate a settings category
- **WHEN** a category tab has focus and the user presses Enter
- **THEN** that category becomes active and its setting rows are displayed

#### Scenario: Confirm a setting
- **WHEN** the user confirms an edited settings value
- **THEN** the value is validated, saved to the client config, and applied immediately using existing config update behavior

### Requirement: Login page interaction
The login Input Page SHALL retain the API key, Account, and Proxy subflows while using shared focus and editor behavior. A provider credential that cannot be written SHALL not offer a writable activation. Existing proxy rows SHALL have an Enter action that opens a deletion confirmation page; the delete message SHALL be sent only after the user activates the explicit Delete confirmation.

#### Scenario: Enter an API key
- **WHEN** the user activates a writable provider, types a key, and confirms it
- **THEN** the client sends the existing API-key update message while displaying the typed secret only as masking glyphs

#### Scenario: Decline proxy deletion
- **WHEN** the user opens deletion confirmation for an existing proxy and activates Cancel or presses Esc
- **THEN** no proxy-delete message is sent and the proxy list remains available

#### Scenario: Confirm proxy deletion
- **WHEN** the user activates Delete on the proxy deletion confirmation page
- **THEN** the client sends the existing `LoginProxyDelete` message for that proxy and returns to the proxy list

### Requirement: Model page interaction
The model Input Page SHALL render providers and models as a transparent two-column page with an Umber divider, governed by one focus. Activating a provider SHALL display that provider's models and move focus to its current or first available model. Activating a model SHALL send the existing model-selection message and close the page. The active model marker SHALL be visually distinct from focus.

#### Scenario: Choose provider and model
- **WHEN** the user activates a provider and then activates one of its models
- **THEN** the client sends `ModelSet` with those IDs and closes the model Input Page

#### Scenario: Provider has no models
- **WHEN** the focused or active provider has an empty model catalog
- **THEN** the page displays an empty-state message and does not expose a fake model focus target

### Requirement: Theme page interaction
The theme Input Page SHALL render each discovered theme as an actionable focus target with its color swatch as non-focusable decoration. Activating a theme SHALL update and persist the configured theme, apply it immediately, and close the page.

#### Scenario: Apply a theme
- **WHEN** the user focuses a discovered theme and presses Enter
- **THEN** that theme becomes the persisted and active theme and the theme Input Page closes

### Requirement: Existing transport compatibility
The Input Page system SHALL use the existing login, proxy, model, and configuration messages, and the effort page SHALL reuse the existing `model-get`/`model-set` wire messages. This change SHALL NOT introduce a new wire message type, but it MAY require the wire protocol-version bump to v6 so peers agree on the new optional `model-set.reasoningEffort` and model `reasoning` fields.

#### Scenario: Send page action
- **WHEN** an Input Page performs a login, proxy, model, or effort operation
- **THEN** the client emits the existing corresponding `ClientMessage` shape (`ModelSet` may carry the optional `reasoningEffort`) without introducing a new wire message type

### Requirement: Non-configuration surfaces remain independent
The session picker, help overlay, Reading View, approval interaction, and question bar SHALL remain outside the Input Page roster. Reading View MUST NOT activate while a blocking Input Page, approval, or question owns input.

#### Scenario: Open the session picker
- **WHEN** the user invokes the existing session-picker action while no Input Page owns input
- **THEN** the existing session picker behavior remains available and is not represented as a configuration Input Page

#### Scenario: Reading View is requested from a blocking page
- **WHEN** a blocking Input Page owns input and the user presses the Reading View binding
- **THEN** the page retains input ownership and Reading View does not activate

### Requirement: Page and composer state survive Reading View
Entering and exiting Reading View SHALL NOT discard or mutate the ordinary composer draft, cursor, multiline state, completion state, transcript state, or inactive Input Page session data.

#### Scenario: Return to the composer
- **WHEN** the user enters Reading View with an unfinished multiline draft and later exits
- **THEN** the same draft, cursor position, multiline state, and completion state are restored

### Requirement: Effort page interaction
The effort Input Page SHALL render the current model's adapter-declared efforts as a single-column list governed by one focus. It SHALL show a loading state until the model catalog arrives, an unavailable state when the current route cannot be resolved or exposes no efforts, and a key-hint footer otherwise.

#### Scenario: Choose an effort
- **WHEN** the user focuses an effort option and presses Enter
- **THEN** the client sends `ModelSet` with the current provider/model and the chosen `reasoningEffort`, and closes the effort Input Page

#### Scenario: No selectable efforts
- **WHEN** the current route exposes no `reasoning.efforts` or cannot be found in the catalog
- **THEN** the page shows an empty/unavailable message and exposes no fake focus target

#### Scenario: Wait for the catalog
- **WHEN** the effort page is open and its requested model catalog has not arrived
- **THEN** the page shows a loading state rather than reporting an empty catalog as a completed result

### Requirement: Model letter marks
Inside `/model`, Shift plus an available ASCII letter SHALL assign that lowercase letter to the focused model's exact provider/model route without selecting it or closing the page. Each letter SHALL identify at most one route and each route SHALL have at most one letter; reassignment SHALL replace both conflicting associations, while repeating the same assignment SHALL remove that mark. Provider-only focus, loading, and empty catalogs SHALL NOT create marks. Marks SHALL persist in shared frontend config across page reopen and process restart, defaulting to none for older configs.

#### Scenario: Mark and reassign a model
- **WHEN** a model has focus and the user presses Shift+A, then assigns A to another model
- **THEN** only the second model retains the saved `a` mark and the page remains open

#### Scenario: Toggle off a mark
- **WHEN** the focused model is already marked `a` and the user presses Shift+A again
- **THEN** the saved mark is removed and the page remains open

#### Scenario: Reopen the menu
- **WHEN** the user reopens `/model` or restarts the frontend after saving a mark
- **THEN** the same provider/model route retains its letter regardless of catalog ordering

### Requirement: Model marks respect effective key mappings
A letter SHALL be unavailable when either its plain or Shift chord has an effective page or global mapping. Existing mappings SHALL retain priority. Reloaded mappings SHALL immediately suppress conflicting mark assignment, selection, and suffixes without deleting saved associations. Ctrl, Alt, Super, other modified chords, non-ASCII letters, and key releases SHALL NOT trigger model marks.

#### Scenario: Reserved navigation letter
- **WHEN** the default page navigation maps `j` and the user presses Shift+J over a model
- **THEN** no mark is assigned and plain `j` retains navigation behavior

#### Scenario: Mapping reload conflicts with a saved mark
- **WHEN** an effective page or global mapping claims the plain or shifted chord of a saved letter
- **THEN** that mark stops acting as a shortcut and is not displayed until both chords become available again

### Requirement: Select and display marked models
A plain available marked letter inside `/model` SHALL send the existing model-selection request for its exact route and close the page, even when another provider is displayed. A missing catalog route or unmarked letter SHALL do nothing and keep the page open. Each available marked model SHALL show ` [<lowercase letter>]` immediately after its name in the theme's Bark-equivalent tone, independent of selection or focus styling; name truncation SHALL reserve space for the suffix when the column can fit it. Help and model-page hints SHALL describe marking and selection.

#### Scenario: Select across providers
- **WHEN** a saved marked route is in the current catalog and the user presses its plain letter while browsing another provider
- **THEN** the client sends ModelSet for the saved provider/model and closes only the model page, preserving the composer draft

#### Scenario: Catalog no longer contains the route
- **WHEN** the user presses the letter of a route absent from the current catalog
- **THEN** no model-selection request is sent and the menu stays open

#### Scenario: Render a marked row
- **WHEN** a marked model is rendered, including as the focused or active model
- **THEN** its name is followed by a Bark-equivalent ` [a]` suffix without inheriting the focus or active color

### Requirement: Model and effort footer separation
The `/model` and `/effort` Input Pages SHALL reserve one blank row between the body and key-hint footer whenever the allocated page height can accommodate the shell and gap. Preferred page height SHALL include this row, and scrolling SHALL exclude it from the visible body capacity. Other Input Pages SHALL retain their existing spacing.

#### Scenario: Content-sized page
- **WHEN** a model or effort page has enough space to display its complete body
- **THEN** one blank row separates the body from the key-hint footer

#### Scenario: Height-capped page
- **WHEN** a model or effort list exceeds the available body height
- **THEN** visible options scroll above the reserved blank row without occupying it or overwriting the footer

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

### Requirement: Protected global page shortcuts
Global choose_model, choose_effort, open_settings and resume_session SHALL default to osmain-l, osmain-e, osmain-comma and osmain-n, reuse existing page actions without rewriting the composer, and SHALL NOT replace an already active page, Reading View or pending approval. Help and transcript paging SHALL remain available without discarding protected state.

#### Scenario: Open a picker from a draft
- **WHEN** the user invokes choose_model from ordinary input
- **THEN** the model page opens while retaining the complete draft

#### Scenario: Pending question
- **WHEN** a question page is awaiting an answer and choose_effort is invoked
- **THEN** the question page and its draft remain active and no page is replaced

### Requirement: Resume title and modification date rows
Resume rows SHALL omit session paths and display a left-aligned title in the theme's Mist-equivalent tone and an available last-modified age right-aligned in its Bark-equivalent tone. The age SHALL express elapsed time from modification to now using integer `d`, `h`, `m`, and `s` units with no spaces or decimal points. It SHALL retain at most the largest unit and its adjacent smaller unit, omit a zero remainder, and discard finer precision. Seconds SHALL be the smallest unit; subsecond and future modification times SHALL display `0s`. Visible ages SHALL refresh as their displayed value changes without rescanning files. A focus marker SHALL identify the selected row without overriding either text tone. Title truncation SHALL reserve room for the age and separate the columns when space permits. A backend that does not provide last-modified metadata SHALL leave the age absent rather than mislabel creation time as modification time. Paths SHALL remain usable for identity filtering and attachment despite being omitted from row presentation.

#### Scenario: Focused dated row
- **WHEN** a Resume row with modification metadata is focused
- **THEN** its title remains Mist-equivalent, its relative age remains Bark-equivalent at the right edge, and a separate marker identifies focus without showing the session path

#### Scenario: Compact relative ages
- **WHEN** sessions were modified five seconds, ten minutes, thirteen hours, or three days and two hours ago
- **THEN** their labels are respectively `5s`, `10m`, `13h`, and `3d2h`, without calendar timestamps or decimal points

#### Scenario: Visible age advances
- **WHEN** an open Resume row's modification age crosses its displayed precision boundary
- **THEN** its relative label refreshes without reloading session metadata or moving selection

#### Scenario: Clock skew or subsecond age
- **WHEN** a session's modification timestamp is in the future or less than one second old
- **THEN** its label is `0s` rather than a negative or fractional duration

#### Scenario: Narrow row
- **WHEN** a title and age cannot fit in the available row width
- **THEN** the title is truncated before the age and neither column wraps or overlaps neighboring rows

#### Scenario: Missing modification date
- **WHEN** a backend provides a session without last-modified metadata
- **THEN** the title remains visible with no fabricated modification age
