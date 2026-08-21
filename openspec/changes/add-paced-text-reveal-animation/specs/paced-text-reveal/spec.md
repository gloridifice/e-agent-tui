## ADDED Requirements

### Requirement: Reusable foreground fade profile
The client SHALL expose one reusable styled-text reveal operation whose fade profile is a single static variable-length array ordered from newest character to oldest character. The initial profile SHALL be `[0.217, 0.53]`. For a profile of length N and a newly revealed character at position `a`, position `a` SHALL use element 0, position `a - 1` SHALL use element 1, and so on while available; position `a - N` and every older position SHALL use its original foreground color.

Each profile value SHALL be interpreted as the foreground proportion in an sRGB-channel interpolation from configured `background_color` to the character's resolved semantic foreground: `display = background + (foreground - background) * weight`. The operation SHALL preserve text, background color, and style modifiers, and SHALL alter only the effective foreground of affected characters.

#### Scenario: Default two-character profile advances
- **WHEN** original foreground text reveals a new character at position `a` with the default profile
- **THEN** `a` is painted with foreground proportion `0.217`, `a - 1` with `0.53`, and `a - 2` is restored to its original foreground

#### Scenario: Profile length changes
- **WHEN** the static profile is changed to contain N values
- **THEN** the same reveal operation affects exactly the newest N revealed characters without requiring surface-specific logic changes

#### Scenario: Mixed semantic styles
- **WHEN** the affected suffix crosses Markdown, diff, ANSI-normalized, or other semantic span boundaries
- **THEN** each character blends toward its own resolved foreground while retaining its original background, bold, italic, underline, and other non-foreground modifiers

### Requirement: Unicode-safe paced reveal
A reveal lane SHALL reveal complete Unicode grapheme clusters and SHALL NOT expose a partial combining sequence or emoji ZWJ sequence. Semantic printable whitespace SHALL count toward the configured character rate; Markdown control syntax and generated layout padding (including code-block background fill) SHALL NOT count because pacing applies to rendered source text rather than raw markup or width-dependent geometry. Structural line boundaries SHALL not consume character budget.

The first available grapheme MAY appear immediately. Subsequent graphemes in the same lane SHALL appear in one visible frame batch no faster than the configured rate permits; rates that exceed one grapheme per 16ms frame SHALL reveal a proportionally sized batch. Every grapheme in that batch SHALL share the same fade-profile position. A delayed scheduler turn SHALL NOT reveal more than one pending batch. Once a finite document has revealed all graphemes, the lane SHALL advance the fade profile at the same cadence until every character has its original foreground, then become inactive.

#### Scenario: Combining sequence reaches the reveal boundary
- **WHEN** the next rendered unit is a base character plus combining marks
- **THEN** the complete grapheme appears in one reveal step and is assigned one fade position

#### Scenario: Scheduler resumes late
- **WHEN** several nominal reveal intervals pass before the client can process the next deadline
- **THEN** that deadline reveals at most one configured frame batch per lane and schedules later batches from the actual processing time rather than displaying elapsed batches at once

#### Scenario: Finite text reaches its end
- **WHEN** the final grapheme has appeared and no later grapheme can arrive for that document
- **THEN** bounded fade-only steps restore the trailing profile characters to their original foreground before reveal scheduling stops

### Requirement: Paced assistant Markdown replies
New live assistant reply blocks rendered as transcript Markdown SHALL queue their complete received text but paint it through the transcript reveal lane at no more than `message_chars_per_second`, defaulting to 120 characters per second. Upstream chunks MAY extend the queued target at any rate without forcing the paint cursor to catch up. Snapshot replay, backward-history prepend, and already historical blocks SHALL be complete immediately rather than replaying old conversations.

#### Scenario: Assistant chunk arrives in a burst
- **WHEN** one live assistant chunk contributes many rendered graphemes at once
- **THEN** the transcript exposes them in rate-sized visible frame batches at no more than the configured message rate, with each batch sharing its fade colors

#### Scenario: Upstream stream is slower than the cap
- **WHEN** a new assistant grapheme arrives after the reveal lane is idle and eligible to paint
- **THEN** it may appear immediately and later graphemes remain subject to the configured maximum rate

#### Scenario: Network stream settles before the visual queue
- **WHEN** the assistant's source stream finishes while unrevealed graphemes remain
- **THEN** the visual queue continues at the configured rate and then drains its fade profile without losing text

#### Scenario: Historical transcript is loaded
- **WHEN** snapshot or history replay materializes an existing assistant Markdown block
- **THEN** its entire rendered content uses original semantic colors immediately and no reveal deadline is started for it

### Requirement: Paced reveal for every textual Preview content kind
Whenever a Ready Preview target becomes visible, all text produced by its `PreviewContent` renderer SHALL pass through the shared reveal operation at no more than `preview_chars_per_second`, defaulting to 300 characters per second. This SHALL include links, diffs, line previews, search results, commands, paths, Markdown, reasoning, muted Markdown, plain text, structured tools and terminal output, and mutation hunks. Empty, loading, and error state labels are not Ready content and SHALL remain immediate.

#### Scenario: Command Preview is selected
- **WHEN** a command or structured tool Preview becomes Ready
- **THEN** its header, command, metrics, and any terminal output appear progressively in display order with the shared fade and Preview rate

#### Scenario: Prompt injection Preview is selected
- **WHEN** muted Markdown prompt-injection content becomes the Ready Preview target
- **THEN** its rendered text reveals progressively while preserving its muted semantic foregrounds and Markdown modifiers

#### Scenario: Preview target identity changes
- **WHEN** selection moves to a different Preview target, including a cached target
- **THEN** the new target starts a fresh Preview reveal, Preview scroll follows existing identity-change rules, and transcript reveal state is unaffected

#### Scenario: Same Preview target receives a revision
- **WHEN** a selected target is enriched or revised with a stable rendered prefix
- **THEN** already revealed common-prefix text remains visible, only the changed suffix is queued, and same-target scroll is preserved

### Requirement: Reveal state does not alter semantic content
Reveal progress SHALL be presentation-only sidecar state. The transcript store, Preview cache, original Markdown source, copy provenance, Reading View content, and event/history reconstruction SHALL retain complete semantic content independent of the current paint cursor. Resizing or theme changes SHALL rematerialize layout/styles and reapply the current logical reveal position without exposing queued text or resetting progress.

#### Scenario: Copy during transcript reveal
- **WHEN** the user copies an assistant block before its visual reveal completes
- **THEN** copy uses the complete original block source rather than only the visible prefix

#### Scenario: Terminal resizes during reveal
- **WHEN** width changes while transcript or Preview text is queued
- **THEN** wrapping is recomputed from the complete semantic content and the same grapheme reveal position is applied to the new layout

#### Scenario: Theme changes during fade
- **WHEN** a theme or `background_color` change is applied while characters are faded
- **THEN** the affected suffix is recomputed from the new resolved foregrounds and background reference without changing reveal progress

#### Scenario: Plain-color mode is active
- **WHEN** `plain_color` suppresses semantic color output
- **THEN** character pacing remains active but foreground interpolation is omitted
