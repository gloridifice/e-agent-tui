# unified-preview-pane Specification

## Purpose
TBD - created by archiving change refactor-client-architecture-and-reading-view. Update Purpose after archive.
## Requirements
### Requirement: One Preview pane across interaction modes
The screen SHALL use one `PreviewPaneState`, one Preview Region, one resolver contract, and one cache in normal mode and Reading View. Normal mode SHALL follow the newest eligible semantic Block; Reading View SHALL follow the current Item when present and otherwise the current Block.

#### Scenario: Newest Block changes in normal mode
- **WHEN** a new eligible Block enters the Reading Document in normal mode
- **THEN** it becomes the Preview target and Preview scroll resets to the top

#### Scenario: Reading cursor is active
- **WHEN** Reading View has both a current Block and Item
- **THEN** the Item becomes the Preview target without creating a mode-specific Preview state

#### Scenario: Exit Reading View
- **WHEN** Reading View exits
- **THEN** Preview policy returns to newest-Block following and resolves the current newest Block

### Requirement: Stable Preview selection under timeline updates
Streaming updates and tool settlement SHALL refresh the same targeted Block when its semantic identity is unchanged. History prepend MUST NOT replace the normal-mode newest target, and new live Blocks MUST NOT steal the target while Reading View is active.

#### Scenario: Streaming extends the latest Block
- **WHEN** assistant streaming extends the latest Block without changing its identity
- **THEN** Preview refreshes that target rather than selecting a new Block

#### Scenario: Older history is prepended
- **WHEN** older Blocks are inserted before the current document in normal mode
- **THEN** the newest pre-existing Block remains selected

#### Scenario: Live output arrives during reading
- **WHEN** a new Block is appended while Reading View is active
- **THEN** the Reading cursor and Preview target remain on the user's selected Block or Item

### Requirement: Complete and themed Preview presentation
Every eligible Block SHALL provide either a specialized Preview or a complete-source fallback. Preview SHALL support link, diff, file lines, search result, command, path, Markdown, plain text, loading, error, and empty presentation without exposing unbounded raw payloads.

The Preview sidebar MAY reuse existing Components or introduce sidebar-specific Components. It MUST use the active theme coherently, MUST NOT alter main-pane style tokens as a side effect, and any new theme token MUST have a default in every bundled theme.

#### Scenario: Block lacks a specialized Preview
- **WHEN** an eligible Block has no specialized Preview mapping
- **THEN** the pane renders the Block's complete copy source rather than an error or blank value

#### Scenario: Transcript is empty
- **WHEN** the Reading Document contains no eligible Blocks
- **THEN** the pane renders its bounded empty state

#### Scenario: Sidebar introduces a new visual primitive
- **WHEN** Preview requires a component not used by the main pane
- **THEN** the component follows active-theme defaults and leaves existing main-pane rendering unchanged

### Requirement: Responsive pane layout
At sufficient width, the Screen SHALL calculate `main_width = min(floor(0.6 * W), width_config)` from usable width `W` and assign remaining columns to Preview, with at least 32 columns for Preview. Below the tested combined minimum width, the default SHALL remain main-only and an explicit toggle SHALL show Preview full-screen rather than rendering two unusably narrow panes.

#### Scenario: Wide terminal renders both panes
- **WHEN** the usable width can satisfy the configured main width and the 32-column Preview minimum
- **THEN** the main pane and full-height Preview pane render in non-overlapping asserted rectangles

#### Scenario: Terminal is too narrow
- **WHEN** usable width is below the tested combined minimum
- **THEN** the Screen renders the main pane without a truncated sidebar and permits Preview to be shown as a full-screen view

#### Scenario: Effective transcript width changes
- **WHEN** entering or leaving a two-pane layout changes main-pane content width
- **THEN** transcript layout and copy provenance invalidate together before visible rows are materialized

### Requirement: Race-safe deferred Preview resolution
A deferred Preview SHALL be requested with a key, revision, and request ID through an owned `UiAction`. Completion SHALL return as an event. A completion MAY populate the shared cache, but it MUST update the visible pane only if its key, revision, and request ID still match the current target.

#### Scenario: Older result arrives after target change
- **WHEN** target A is requested, target B is selected, and result A arrives first
- **THEN** result A may be cached but target B remains visible

#### Scenario: Cached target is revisited
- **WHEN** the user returns to a target whose matching revision is cached
- **THEN** Preview reuses the cached value without requiring duplicate resolution

#### Scenario: Resolver awaits I/O
- **WHEN** `e-dsh` resolves a file, diff, or kernel-backed Preview
- **THEN** no UI state guard is held while it awaits and completion requests a draw through the existing scheduler

