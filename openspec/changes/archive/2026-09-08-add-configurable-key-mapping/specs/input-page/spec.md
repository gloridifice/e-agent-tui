## MODIFIED Requirements

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

### Requirement: Browse and text-edit modes
The Input Page system SHALL distinguish browse, choice-edit, and text-edit contexts. In browse mode configured navigation SHALL move focus; in text-edit mode printable characters including h/j/k/l/q SHALL be inserted into the editor. Configurable edit confirm and cancel SHALL default to Enter and Esc and SHALL not pass to ordinary input or enclosing page navigation. Resume filtering and free-text questions SHALL not inherit browse letter navigation.

#### Scenario: Type Vim navigation letters
- **WHEN** a text or secret editor is active and the user types hjklq
- **THEN** those five characters are added to the editor and page focus does not move

#### Scenario: Cancel an edit
- **WHEN** the user invokes edit cancel while an editor is active
- **THEN** the unconfirmed value is discarded and the page remains open at the edited element

## ADDED Requirements

### Requirement: Protected global page shortcuts
Global choose_model, choose_effort, open_settings and resume_session SHALL default to osmain-l, osmain-e, osmain-comma and osmain-n, reuse existing page actions without rewriting the composer, and SHALL NOT replace an already active page, Reading View or pending approval. Help and transcript paging SHALL remain available without discarding protected state.

#### Scenario: Open a picker from a draft
- **WHEN** the user invokes choose_model from ordinary input
- **THEN** the model page opens while retaining the complete draft

#### Scenario: Pending question
- **WHEN** a question page is awaiting an answer and choose_effort is invoked
- **THEN** the question page and its draft remain active and no page is replaced
