## MODIFIED Requirements

### Requirement: Event-authored mutation Preview
Edit, replace, and insert calls SHALL use mutation content already supplied by their events. The client MUST NOT read the target file, compare before/after files, run an LCS/diff algorithm, or invent removed/context lines. Event-provided unified diff text SHALL retain every event-authored row in order; the client MAY classify those rows to present event-authored file paths, hunk coordinates, old/new line numbers, diff structure, and syntax-colored code bodies. Structured old/new fragments MAY be linearly rendered as removed and added rows. Diff syntax tokens SHALL use `semantics.markdown`, while added/removed backgrounds, accents, gutters, and separators SHALL use `semantics.diff`.

#### Scenario: DSH edit provides applied contextual hunks
- **WHEN** a completed edit result carries ordered `meta.diffs` hunks
- **THEN** Preview renders those DSH-computed hunks in order, syntax-highlights code when an event-authored path resolves a grammar, and replaces any less-specific pending call hunk

#### Scenario: str_replace_editor provides requested replacement
- **WHEN** `str_replace_editor` emits `command: "str_replace"` with `old_str` and `new_str` but no result-time applied hunk
- **THEN** Preview renders the event-provided requested replacement and retains it after settlement without claiming extra applied context

#### Scenario: Insert provides only inserted text
- **WHEN** an insert call supplies `new_str` and `insert_line` without a before-image
- **THEN** Preview renders an addition-only hunk anchored to that line and does not synthesize removed or surrounding lines

#### Scenario: Mutation payload is incomplete
- **WHEN** an edit, replace, or insert event lacks the data required by its safe mutation presentation
- **THEN** Preview falls back to its path or bounded generic tool presentation rather than calculating a diff

#### Scenario: Create remains common-format
- **WHEN** a create call includes complete new-file content
- **THEN** Preview renders `create` plus the path and does not switch to an all-added diff

#### Scenario: Event-provided unified diff is malformed
- **WHEN** a unified diff cannot be fully classified for line numbers or syntax selection
- **THEN** Preview retains all event-authored rows with fallback semantic styling and does not invent, reorder, or drop mutation content

### Requirement: Muted Markdown for injected context
Prompt-injection/context Preview SHALL use complete Markdown rendering through `semantics.markdown_weak`, whose role set matches normal Markdown while its bundled mappings are limited to Bark, Umber, and Night equivalents. It SHALL preserve Markdown and syntax-token bold, italic, and underline modifiers. Reasoning/thinking Preview behavior SHALL remain distinct in semantic content kind while using the same weak Markdown presentation context.

#### Scenario: Injected instructions contain Markdown
- **WHEN** an injected context message contains headings, bold, italic, code, links, or a language-labelled fenced code block
- **THEN** Preview renders the complete Markdown through weak semantic roles and preserves the corresponding modifiers and syntax distinctions

#### Scenario: Reasoning follows an injected context
- **WHEN** Preview later targets a reasoning/thinking block
- **THEN** it retains the reasoning semantic content kind and uses weak Markdown presentation rather than the injected-context content kind

#### Scenario: Normal transcript Markdown has the same source
- **WHEN** source rendered weakly for injected-context Preview also appears as transcript Markdown
- **THEN** the transcript rendering continues to use unchanged `semantics.markdown` roles
