## MODIFIED Requirements

### Requirement: Shared display-row layout and scroll coordinates
The client SHALL derive viewport selection, mouse and page scrolling, follow mode, history prepend anchoring, Reading View geometry, Block/Item overlays, and copy provenance from one width-specific display-row layout. The layout SHALL account for wrapping and Unicode display width, SHALL invalidate when pane content width or relevant base lines change, and SHALL materialize or clone only rows needed by the visible window except for lightweight row-count and semantic-geometry indexes.

#### Scenario: Wheel scroll crosses wrapped paragraphs
- **WHEN** one wheel notch scrolls through content containing wrapped ASCII or CJK lines
- **THEN** the viewport moves exactly three display rows rather than three unwrapped cache lines

#### Scenario: Page scroll uses the visible transcript height
- **WHEN** the user presses PageUp or PageDown
- **THEN** the viewport moves by the current visible transcript height minus one display row regardless of source-line wrapping

#### Scenario: Terminal or pane width changes
- **WHEN** resize or responsive pane layout changes transcript content width
- **THEN** old wrapped-row counts, semantic geometry, and materialized rows are invalidated before viewport, Reading cursor, and copy coordinates are calculated for the new width

#### Scenario: Older history is prepended
- **WHEN** history loading inserts effective content above a non-following viewport
- **THEN** the offset increases by the exact number of newly inserted display rows so the previously visible content remains anchored

#### Scenario: Reading cursor crosses wrapped content
- **WHEN** Reading View navigates a wrapped Markdown, code, table, Mermaid, user, reasoning, or tool Block
- **THEN** Block and Item geometry matches rendered display rows while copied text still comes from original source provenance

#### Scenario: Follow mode receives a wrapped tail
- **WHEN** new streaming content wraps onto additional rows while follow mode is enabled
- **THEN** the viewport remains pinned to the final display rows and preserves the trailing gap before the input area

## ADDED Requirements

### Requirement: Preview and Reading work remains independently bounded
Preview target changes, Preview scrolling, Reading cursor movement, and deferred Preview completion SHALL invalidate only affected Preview or overlay ranges and MUST NOT reparse Markdown or rebuild the complete transcript. Normal latest-Block streaming SHALL patch only the changed transcript tail and matching Preview value when structurally possible.

#### Scenario: Reading cursor moves between cached Blocks
- **WHEN** the user navigates between Blocks whose layouts and Preview values are cached
- **THEN** the client updates cursor overlays and Preview selection without reparsing or rematerializing unrelated transcript Blocks

#### Scenario: Deferred Preview completes
- **WHEN** a matching asynchronous Preview result arrives
- **THEN** it updates the Preview cache and requests one scheduled draw without structurally invalidating transcript layout

#### Scenario: Two-pane frame materializes rows
- **WHEN** a frame renders a long transcript and long Preview at wide terminal size
- **THEN** it materializes only visible rows for each pane and retains lightweight total-row indexes for scrolling

### Requirement: Performance gates cover the two-pane experience
Release benchmarks and logical cache tests SHALL cover wide normal-mode Preview updates, Reading cursor movement, continuous streaming, scrolling, animation, and responsive width changes while retaining the existing P95 complete-frame redline and bounded-work assertions.

#### Scenario: Preview benchmark runs
- **WHEN** the release frame benchmark exercises normal latest-Block Preview and Reading navigation at documented terminal sizes
- **THEN** it reports complete-frame latency, transcript rebuild/patch counts, Preview rebuild/patch counts, changed cells, and emitted bytes

#### Scenario: Logical tests run on variable hardware
- **WHEN** ordinary tests exercise Preview and Reading updates outside the reference benchmark machine
- **THEN** they assert bounded invalidation and cache behavior rather than wall-clock timing alone
