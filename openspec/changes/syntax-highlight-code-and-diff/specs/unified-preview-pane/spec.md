## MODIFIED Requirements

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
