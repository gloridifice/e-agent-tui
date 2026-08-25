## MODIFIED Requirements

### Requirement: Ordinary transcript block contract
A transcript block SHALL render non-status content using a declared content format and tone. It SHALL support plain notices, Markdown source, reasoning content, and bounded unknown-surface fallback content while preserving the original copy source and semantic Block annotations.

#### Scenario: Assistant final message renders Markdown
- **WHEN** a finalized assistant message contains text blocks
- **THEN** one ordinary transcript block renders the combined text as Markdown and exposes the original Markdown through its Reading Block copy payload

#### Scenario: Turn outcome renders a notice
- **WHEN** a turn ends as aborted, blocked, interrupted, max-tokens, or error
- **THEN** an ordinary transcript block presents the appropriate notice or error detail without a running status

#### Scenario: Unknown append surface is bounded
- **WHEN** the client receives an unknown event carrying append-surface metadata
- **THEN** it renders a bounded fallback transcript block that identifies the event without dumping an unbounded payload

### Requirement: Shared layout owns copy provenance
Every transcript display surface SHALL produce visible rows, semantic Reading geometry, and copy provenance through the same width-aware layout process. Atomic Markdown, table, code, Mermaid, and future rich-detail bodies SHALL retain their existing atomic copy semantics.

#### Scenario: Wrapped card copy follows visible layout
- **WHEN** a padded content card wraps across multiple terminal rows
- **THEN** Reading View Block and Item geometry uses exactly those rendered rows while copied text comes from the card's original source

#### Scenario: Activity composition has stable spacing
- **WHEN** adjacent activity rows or an activity row with a detail card are rendered
- **THEN** spacing is produced by the shared layout and Reading View does not independently infer gaps

## ADDED Requirements

### Requirement: Display surfaces provide semantic reading annotations
The four shared display surfaces SHALL remain the only public transcript/input presentation paths while exposing stable semantic Block boundaries, complete copy payloads, and Item annotations required by Reading View and Preview. Reading metadata MUST NOT introduce a fifth event-specific rendering surface.

#### Scenario: Activity provides Reading metadata
- **WHEN** a normalized tool activity is projected
- **THEN** its existing activity/card composition supplies one stable tool Block and any adapter-provided Items without bypassing the shared layout path

#### Scenario: Markdown produces semantic units
- **WHEN** a transcript block renders paragraphs, code, list rows, Mermaid, or links
- **THEN** the existing Markdown/provenance pipeline emits corresponding Block and Item annotations with complete source copy payloads

### Requirement: Main-pane surface styling survives pane extraction
Moving shared display surfaces into `e-tui` Regions and placing them in the main pane SHALL preserve their current spacing, backgrounds, tones, wrapping rules, atomic provenance, and animation behavior at equivalent effective widths. Preview-specific styling MUST NOT change the established main-pane surface contracts.

#### Scenario: User card renders after extraction
- **WHEN** a direct user message is rendered in the extracted main pane at the same content width and theme
- **THEN** its gutter, padding, continuous wrapped background, visible content, and verbatim copy source match the characterization baseline

#### Scenario: Sidebar adds a specialized card
- **WHEN** Preview uses a sidebar-specific shell or detail Component
- **THEN** ordinary transcript cards continue to use their existing main-pane treatment
