## MODIFIED Requirements

### Requirement: Paced reveal for every textual Preview content kind
Whenever a Ready Preview target becomes visible for the first time as live content, all non-reasoning text produced by its `PreviewContent` renderer SHALL be revealed as one complete block and share one initial fade group. Live `Reasoning` content SHALL instead be wrapped to the current Preview content width and revealed by terminal display row at no more than `preview_lines_per_second`, defaulting to 30 rows per second. This distinction SHALL cover links, diffs, line previews, search results, commands, paths, Markdown, reasoning, muted Markdown, plain text, structured tools and terminal output, and mutation hunks. Empty, loading, and error state labels are not Ready content and SHALL remain immediate.

One reasoning pacing unit SHALL be one non-empty wrapped display row. Structural empty rows SHALL attach to adjacent visible content and SHALL NOT consume a row-rate step by themselves. Every styled grapheme in a newly revealed row or block SHALL share that unit's fade age while retaining its own semantic foreground, background, and modifiers.

Previously materialized content selected through replay, resume, cache revisit, or Reading View SHALL reveal the complete current page as one fade group rather than replaying first-appearance row pacing. Same-target revisions SHALL preserve their stable visible prefix and SHALL NOT restart first-appearance pacing. When `preview_lines_per_second` is zero, all content SHALL be visible immediately with original semantic foregrounds.

#### Scenario: New live reasoning Preview is selected
- **WHEN** a fresh live reasoning target contains multiple wrapped display rows and `preview_lines_per_second` is 30
- **THEN** the first row may appear immediately and subsequent rows appear one at a time at intervals of approximately one thirtieth of a second

#### Scenario: Long reasoning line wraps in Preview
- **WHEN** one fresh live reasoning source line occupies several terminal rows at the current width
- **THEN** each wrapped display row consumes a separate pacing unit rather than all rows appearing as one logical-line batch

#### Scenario: New live tool Preview is selected
- **WHEN** a fresh command or structured tool Preview becomes Ready
- **THEN** its header, primary information, terminal output, and mutation content appear together as one block fade rather than progressively by row

#### Scenario: Preview is selected from history or Reading View
- **WHEN** replay, resume, a cached revisit, or Reading View selects a Ready target
- **THEN** the complete current Preview page appears together as one fade group regardless of content kind

#### Scenario: Same Preview target receives a revision
- **WHEN** a selected target is enriched or revised with a stable rendered prefix
- **THEN** the common prefix remains visible, newly introduced non-reasoning content is admitted as one block, and same-target scroll is preserved

#### Scenario: Preview width changes during reasoning reveal
- **WHEN** Preview width changes after some reasoning display rows have appeared
- **THEN** current row boundaries are recomputed while the semantic grapheme frontier remains unchanged until another row step becomes due

#### Scenario: Preview pacing is disabled
- **WHEN** `preview_lines_per_second` is zero
- **THEN** all wrapped rows are visible immediately with original semantic foregrounds

## ADDED Requirements

### Requirement: Streaming Markdown code-block reveal is geometrically stable
Append-only live assistant Markdown updates SHALL NOT move the already painted reveal frontier backward solely because generated code-block presentation, including its line-count header and fill padding, changed during rematerialization. Generated code-block fill padding MUST remain outside the semantic reveal budget, and the complete settled source and styles SHALL remain unchanged.

#### Scenario: Code block gains source lines while streaming
- **WHEN** an open fenced code block grows from one line to several lines and its generated line-count header changes
- **THEN** already painted header and code rows are not hidden and replayed before the newly admitted tail appears

#### Scenario: Code block settles
- **WHEN** the closing fence settles a streaming code block
- **THEN** the final header, syntax-highlighted rows, background fill, and complete source render normally without an early-row reveal restart

#### Scenario: Non-append Markdown replacement occurs
- **WHEN** a Markdown block is replaced rather than extended append-only
- **THEN** reveal reconciliation may return to the actual common semantic prefix instead of preserving an invalid frontier
