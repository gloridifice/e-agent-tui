# syntax-highlighted-code Specification

## Purpose
TBD - created by archiving change syntax-highlight-code-and-diff. Update Purpose after archive.
## Requirements
### Requirement: Language-aware fenced code presentation
The client SHALL syntax-highlight the body of a fenced Markdown code block when its normalized language token resolves to an embedded syntax. It SHALL retain the existing code header, atomic ownership, raw-source provenance, collapse behavior, complete-source copy behavior, and code-block background independently of token styling.

#### Scenario: Known Rust fence renders in the transcript
- **WHEN** transcript Markdown contains a fenced block whose language token resolves to Rust
- **THEN** its keywords, types, strings, comments, and other recognized scopes render as styled spans while the block retains its existing header, background, and atomic copy source

#### Scenario: Code block is collapsed
- **WHEN** a highlighted code block exceeds the configured atomic collapse threshold
- **THEN** the same head, collapse hint, and tail rows are shown and copying or expanding still addresses the complete original fenced block

#### Scenario: Fence is unknown or unlabelled
- **WHEN** a fenced code block has no resolvable language token
- **THEN** its complete body renders with the selected Markdown `code_text` semantics rather than failing or disappearing

### Requirement: Context-sensitive Markdown syntax semantics
Syntax token styles SHALL be derived from the active project's Markdown semantic roles rather than a fixed external syntax theme. Transcript code fences and diff bodies SHALL derive token styles from `semantics.markdown`; code fences inside Preview Markdown SHALL derive token styles from `semantics.markdown_weak`. Token mapping SHALL transfer foreground, bold, italic, and underline while code-token backgrounds remain owned by the containing code block or diff row.

#### Scenario: Transcript and Preview render the same fence
- **WHEN** the same fenced source is rendered once in transcript Markdown and once inside Preview Markdown
- **THEN** both use the same syntax-scope mapping but resolve token styles from normal and weak Markdown semantics respectively

#### Scenario: Markdown role has a background
- **WHEN** a syntax scope maps to a Markdown role such as `heading1` or `inline_code` that defines a background
- **THEN** the token ignores that role background and retains the containing code block or diff row background

#### Scenario: Theme changes during Preview reveal
- **WHEN** the active theme changes while syntax-highlighted Preview rows are partly revealed or faded
- **THEN** their foregrounds and modifiers are rematerialized from the new semantic group without changing semantic content, target identity, or reveal progress

### Requirement: Syntax-colored event-authored diff bodies
The client SHALL classify event-authored unified diff rows and structured old/new fragments for presentation without calculating a diff. When a path or event-authored file header resolves a syntax, removed code SHALL be highlighted in an old logical stream and added code SHALL be highlighted in a new logical stream. Syntax foregrounds and modifiers SHALL compose with project-owned diff gutters, line numbers, accents, and full-row backgrounds.

#### Scenario: Rust replacement has old and new rows
- **WHEN** an event-authored mutation for a `.rs` path contains removed and added Rust code
- **THEN** both bodies receive Rust syntax foregrounds, removed rows retain the removed background and old coordinates, and added rows retain the added background and new coordinates

#### Scenario: Multiline state crosses rows in one hunk
- **WHEN** consecutive event-authored diff rows contain a multiline comment or string recognized by the selected grammar
- **THEN** highlighting preserves parser state within the corresponding old or new logical stream without interleaving mutually exclusive removed and added code

#### Scenario: Unified diff contains multiple file sections
- **WHEN** one event-authored unified diff contains file headers for different extensions
- **THEN** each classified section uses its own event-authored path hint and every source row remains in display order

#### Scenario: Diff is malformed or has no syntax hint
- **WHEN** diff text cannot be fully classified or no path resolves a syntax
- **THEN** the client renders all retained event-authored text with safe semantic fallback styling and does not synthesize context or edit rows

### Requirement: Required weak Markdown theme semantics
The fixed theme schema SHALL define `semantics.markdown_weak` with exactly the same semantic roles as `semantics.markdown`. Every bundled theme SHALL map weak Markdown roles only to its Bark-, Umber-, and Night-equivalent palette entries and MAY retain bold, italic, and underline modifiers. A custom theme that omits any required weak Markdown role SHALL be rejected through the existing whole-theme validation path.

#### Scenario: Bundled theme renders weak Markdown
- **WHEN** Ferra or DeepSeek E renders Preview Markdown
- **THEN** every Markdown foreground and background comes from Bark, Umber, or Night equivalents while supported modifiers distinguish semantic structure

#### Scenario: Existing normal Markdown is rendered
- **WHEN** transcript Markdown is materialized after the theme schema change
- **THEN** all existing `semantics.markdown` mappings remain unchanged

#### Scenario: Custom theme omits the weak group
- **WHEN** a discovered custom theme has the prior schema and lacks `semantics.markdown_weak`
- **THEN** the complete custom theme is considered illegal and existing theme fallback behavior applies

### Requirement: Bounded and cached highlighting
Syntax assets SHALL be embedded and initialized at most once without render-time filesystem I/O. Highlighting SHALL enforce bounded source, row, and individual-line limits and SHALL cache styled layouts at the owning transcript or Preview presentation layer. Highlighting failure or a limit breach MUST degrade to semantic plain-code styling without changing complete semantic or copy content.

#### Scenario: Unchanged Preview redraws
- **WHEN** syntax-highlighted Preview content is redrawn for reveal fade, scrolling, mouse selection, or an unrelated frame request without a key, revision, width, or theme-style change
- **THEN** the client reuses its styled layout and does not rerun syntax parsing

#### Scenario: One streaming code block grows
- **WHEN** a live assistant update extends one fenced code block
- **THEN** syntax rematerialization remains within the existing affected transcript suffix and does not reparse unrelated settled transcript blocks

#### Scenario: Highlight input exceeds a limit
- **WHEN** a code fence or diff body exceeds the configured highlight byte, row, or individual-line limit
- **THEN** its complete retained source remains renderable and copyable with fallback code styling and no unbounded syntax operation

#### Scenario: Syntax engine reports an error
- **WHEN** a grammar or regex operation fails for retained source
- **THEN** the owning code or diff presentation falls back locally and the Preview does not become an error state

### Requirement: Syntax styles remain presentation-only
Syntax highlighting SHALL alter only styled presentation lines. Transcript source, Preview semantic values, Reading Blocks, copy payloads, diff event facts, and reveal semantic frontiers MUST remain complete and width-independent.

#### Scenario: Copy during code reveal
- **WHEN** the user copies a fenced code Block before all highlighted transcript or Preview text is visible
- **THEN** the clipboard action uses the complete original fenced source rather than visible or highlighted terminal spans

#### Scenario: Preview width changes
- **WHEN** a highlighted diff or Preview Markdown code block rewraps after resize
- **THEN** width-dependent styled rows are rematerialized while the semantic source and reconciled reveal frontier remain unchanged

