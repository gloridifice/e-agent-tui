## ADDED Requirements

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
