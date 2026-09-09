# unified-preview-pane Specification

## Purpose
Define shared Preview selection, responsive pane presentation, independent scrolling, and race-safe content resolution across normal interaction and Reading View.

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

The Preview sidebar MAY reuse existing Components or introduce sidebar-specific Components. It MUST use the active theme coherently, MUST NOT alter main-pane style tokens as a side effect, and any new theme token MUST have a default in every bundled theme. Markdown rendered inside Preview, including its fenced code blocks, SHALL use `semantics.markdown_weak`. Diff is a content-type exception: diff code tokens SHALL use `semantics.markdown` while diff row structure continues to use `semantics.diff`.

#### Scenario: Block lacks a specialized Preview
- **WHEN** an eligible Block has no specialized Preview mapping
- **THEN** the pane renders the Block's complete copy source rather than an error or blank value

#### Scenario: Transcript is empty
- **WHEN** the Reading Document contains no eligible Blocks
- **THEN** the pane renders its bounded empty state

#### Scenario: Sidebar introduces a new visual primitive
- **WHEN** Preview requires a component not used by the main pane
- **THEN** the component follows active-theme defaults and leaves existing main-pane rendering unchanged

#### Scenario: Preview renders Markdown with code
- **WHEN** a Preview target contains headings, links, emphasis, and a fenced code block
- **THEN** all Markdown roles and syntax token mappings resolve through `semantics.markdown_weak` without changing the corresponding transcript Markdown styles

#### Scenario: Preview renders a diff
- **WHEN** a Preview target contains a syntax-highlighted event-authored diff
- **THEN** code foregrounds and modifiers resolve through `semantics.markdown` and compose with the `semantics.diff` row background, gutter, and separator roles

### Requirement: Responsive pane layout
At sufficient width, the Screen SHALL calculate message width from the validated committed message-pane percentage and assign the remaining columns to Preview. The message share MUST NOT be less than 25%, and Preview SHALL render beside it only when the resulting Preview rectangle is at least 19 columns wide, providing a separator column, a one-column gap, 16 usable content columns, and a one-column right margin. Otherwise normal presentation SHALL remain main-only with a separator grip in the right margin, and the existing explicit toggle SHALL show Preview full-screen rather than rendering an unusably narrow pane. A responsive collapse caused by terminal width MUST preserve the committed percentage. Main page content SHALL use one-column ordinary horizontal margins; Main-only mode SHALL additionally reserve the collapsed grip geometry.

#### Scenario: Wide terminal renders both panes
- **WHEN** the committed percentage leaves at least 19 columns for the split Preview rectangle
- **THEN** the message and full-height Preview panes render in non-overlapping asserted rectangles calculated from that percentage

#### Scenario: Terminal is too narrow
- **WHEN** usable width is below the tested combined minimum
- **THEN** the Screen renders the main pane without a truncated sidebar and permits Preview to be shown as a full-screen view

#### Scenario: Committed percentage reaches the message minimum
- **WHEN** the committed or pending message share is 25%
- **THEN** message width is calculated as 25% of usable width and no interaction can reduce it further

#### Scenario: Preview would be too narrow
- **WHEN** percentage calculation leaves fewer than 19 columns for the split Preview rectangle
- **THEN** the Screen renders main-only with the right-margin separator grip and permits Preview to be shown as a full-screen view

#### Scenario: Responsive collapse later regains width
- **WHEN** terminal width first forces the split Preview rectangle below 19 columns and later grows enough to satisfy the threshold at the unchanged committed percentage
- **THEN** the split Preview reappears automatically without a persisted config change

#### Scenario: Effective transcript width changes
- **WHEN** entering, leaving, or committing a resized two-pane layout changes message-pane content width
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

### Requirement: Pane separator preserves omitted-background transparency
The pane separator SHALL render its semantic foreground without substituting the themed base-surface color when the active separator bar or drag-guide role omits a background. An omitted separator background SHALL use terminal transparency, while an explicitly configured separator background SHALL remain authoritative. Drag placeholder boxes SHALL continue to use their independent placeholder background role.

#### Scenario: Separator bar omits its background
- **WHEN** the active theme defines a separator bar foreground but no bar background
- **THEN** the idle separator glyph uses the semantic foreground and a terminal-reset background

#### Scenario: Separator drag guide omits its background
- **WHEN** the active theme defines a separator line foreground but no line background and the user drags the separator
- **THEN** the full-height guide and grip retain terminal-reset backgrounds

#### Scenario: Separator background is explicit
- **WHEN** the active separator role defines a background
- **THEN** the corresponding separator glyphs use that configured background

### Requirement: Normal-mode Preview eligibility and reconciliation
Normal-mode automatic Preview following SHALL ignore plain transcript blocks, including frontend system and error messages, and SHALL ignore user cards and user attachments. It SHALL continue to consider specialized context, reasoning, tool, activity, and unknown-surface content eligible. Reading View SHALL retain explicit Preview access to its selected Block or Item regardless of normal-mode automatic eligibility.

#### Scenario: Plain or user content is appended
- **WHEN** a plain block, user card, or user attachment is appended after an eligible normal-mode Preview target
- **THEN** the current target, revision, scroll, and reveal state remain unchanged

#### Scenario: Only ignored content exists
- **WHEN** the transcript contains only plain blocks, user cards, user attachments, or assistant Markdown
- **THEN** normal-mode Preview renders its empty state

#### Scenario: Ignored content is selected in Reading View
- **WHEN** Reading View explicitly selects a plain or user-owned Block
- **THEN** Preview renders that selected Block through the existing complete-source behavior

#### Scenario: Direct command activity settles
- **WHEN** a direct command-result updates the activity that owns the current normal-mode Preview target
- **THEN** Preview refreshes the same target identity to the activity's settled content without waiting for another timeline event

### Requirement: Wheel scrolling follows the pointed pane
Wheel input SHALL preserve terminal cell coordinates and scroll only the pane containing those coordinates, by three display rows per notch. Split-pane separator cells and coordinates outside the viewport SHALL not scroll either pane. Main-only presentation SHALL scroll Main; Preview-only presentation SHALL scroll Preview. An active History page SHALL retain Main ownership and narrow-layout precedence. Wheel input during captured separator resizing SHALL not scroll hidden pane content.

#### Scenario: Pointer moves between split panes
- **WHEN** the user turns the wheel over Main and then over Preview
- **THEN** each event changes only its pointed pane, without requiring a click or changing keyboard focus or Preview target

#### Scenario: History is open beside Preview
- **WHEN** the user turns the wheel over Preview while History occupies Main
- **THEN** Preview scrolls without moving History or the hidden transcript

#### Scenario: Single pane is visible
- **WHEN** responsive layout shows only Main or only Preview
- **THEN** wheel input within the viewport scrolls only the visible pane

#### Scenario: Pointer is on separator or outside the viewport
- **WHEN** a wheel event targets the separator column or an out-of-viewport coordinate
- **THEN** neither pane scrolls

### Requirement: Preview manual scrolling has bounded independent state
Preview wheel scrolling SHALL move from its currently presented tail or manual viewport and clamp between the first row and last full viewport. Manual row zero SHALL remain distinct from automatic tail following. Scrolling back down to the tail SHALL restore automatic following, including pinned command information. Same-target updates SHALL preserve manual review; target identity replacement SHALL reset manual review. Scroll-only frames SHALL reuse styled layout and not invalidate transcript or semantic Preview caches.

#### Scenario: Scroll upward from automatic tail
- **WHEN** overflowing Preview follows the latest content and the user scrolls upward
- **THEN** Preview moves three rows upward from the tail, clamped at row zero

#### Scenario: Reach either boundary
- **WHEN** repeated wheel input reaches the top or bottom
- **THEN** no blank overscroll occurs, row zero remains reachable, and reaching the bottom restores following

#### Scenario: Content updates during manual review
- **WHEN** the selected target receives a same-identity update while Preview is manually scrolled
- **THEN** its manual anchor is retained within the available row bounds
