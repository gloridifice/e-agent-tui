# input-page Specification

## Purpose
TBD - created by archiving change unify-input-pages. Update Purpose after archive.
## Requirements
### Requirement: Unified Input Page lifecycle
The client SHALL represent `/settings`, `/login`, `/model`, and `/theme` as mutually exclusive Input Pages, with at most one Input Page active at a time. Opening one of these commands SHALL replace the ordinary input area without discarding its text or transcript state.

#### Scenario: Open a configuration command
- **WHEN** the user executes `/settings`, `/login`, `/model`, or `/theme`
- **THEN** the corresponding Input Page becomes the sole active configuration page and replaces the ordinary input area

#### Scenario: Close a page
- **WHEN** the active Input Page is closed
- **THEN** the ordinary input area returns with its prior buffer and the transcript and scroll state unchanged

### Requirement: Shared replacement layout
Every Input Page SHALL render without a floating window or border in the bottom page area used in place of the input bar. It SHALL use exactly one blank row of top and bottom inner padding and exactly two blank columns of left and right inner padding around all page content.

#### Scenario: Render a page at normal terminal size
- **WHEN** any Input Page is rendered
- **THEN** its background fills the replacement area, its content begins after two horizontal columns and one vertical row of padding, and no border or centered overlay is drawn

#### Scenario: Render on a small terminal
- **WHEN** an Input Page is rendered in a terminal too small for all body rows
- **THEN** layout calculations remain bounded, visible content is clipped or scrolled, and rendering does not panic or overlap the status and title rows

### Requirement: Single actionable focus
When an Input Page has at least one enabled actionable element, it SHALL expose exactly one visibly focused actionable element. When no enabled actionable element exists, it SHALL expose no focus target. Direction keys and `h/j/k/l` SHALL move focus through enabled actionable elements according to the page focus graph, and Enter SHALL execute only the focused element. Read-only, informational, unavailable, and disabled elements SHALL not receive actionable focus.

#### Scenario: Navigate with equivalent keys
- **WHEN** the user presses an arrow key or its corresponding `h/j/k/l` key in browse mode
- **THEN** focus moves to the same neighboring actionable element for both key forms

#### Scenario: Activate focused element
- **WHEN** the user presses Enter in browse mode
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
The Input Page system SHALL distinguish browse mode from text-edit mode. In browse mode `h/j/k/l` SHALL navigate; in text-edit mode printable characters including `h/j/k/l` SHALL be inserted into the editor. Enter SHALL confirm an edit and Esc SHALL cancel it without passing either key to the ordinary input or enclosing page navigation.

#### Scenario: Type Vim navigation letters
- **WHEN** a text or secret editor is active and the user types `hjkl`
- **THEN** those four characters are added to the editor and page focus does not move

#### Scenario: Cancel an edit
- **WHEN** the user presses Esc while an editor is active
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
The model Input Page SHALL render providers and models as a borderless two-column page governed by one focus. Activating a provider SHALL display that provider's models and move focus to its current or first available model. Activating a model SHALL send the existing model-selection message and close the page. The active model marker SHALL be visually distinct from focus.

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

