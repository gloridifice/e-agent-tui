## ADDED Requirements

### Requirement: Streaming transcript admits only a stable wrap prefix
A live assistant Markdown lane SHALL retain complete semantic rendered content while admitting only a stable grapheme prefix to character pacing. The stable prefix SHALL be derived from the same Unicode line-breaking and grapheme rules as the production wrapper. The open trailing wrap atom, its deferred separator, and the incomplete final row of an over-wide atom SHALL remain hidden until a later break opportunity, bounded timeout, or source settlement makes them eligible.

#### Scenario: Latin word grows past the remaining row width
- **WHEN** a streaming suffix extends the same Latin word until canonical greedy wrapping moves that word to the next display row
- **THEN** the word remains behind the admission frontier until its placement is known and no already painted transcript row moves because of the extension

#### Scenario: CJK closing punctuation arrives after an ideograph
- **WHEN** a trailing CJK ideograph is followed by punctuation that the Unicode line-breaking rules glue to it
- **THEN** the ideograph and punctuation are admitted as one stable trailing wrap atom rather than painting punctuation at the start of a row

#### Scenario: Over-wide atom continues streaming
- **WHEN** an unbroken URL or word exceeds the available display width
- **THEN** completed hard-wrapped rows are admitted while only its incomplete final row remains held

#### Scenario: Streaming source settles
- **WHEN** the assistant block changes from streaming to finite while a rendered tail is held
- **THEN** the complete held tail is admitted immediately to ordinary character pacing without waiting for a timeout

#### Scenario: Held tail becomes idle
- **WHEN** the rendered signature does not change for 100ms or the same tail has been held for 300ms
- **THEN** the current held tail is admitted to character pacing so a slow or unbroken stream cannot starve visible output

### Requirement: Foreground fading has an independent frame clock
Every newly revealed transcript grapheme batch or Preview row batch SHALL create a fade group at profile index zero. Active fade groups SHALL advance at the safe animation-frame cadence independently of content pacing and independently of whether the semantic source is finite. A lane SHALL have no fade deadline after every visible group has returned to its original foreground.

#### Scenario: Streaming queue temporarily empties
- **WHEN** all currently admitted transcript graphemes are visible while the assistant block is still streaming
- **THEN** the newest groups continue through every remaining fade profile color and restore their semantic foreground before the lane becomes idle

#### Scenario: More source arrives after fade becomes idle
- **WHEN** a live lane has restored all colors and later receives another admitted grapheme
- **THEN** the new grapheme starts a new fade group without replaying or recoloring the completed prefix

#### Scenario: Reveal and fade deadlines coincide
- **WHEN** one existing fade group and one new content batch are due in the same scheduler turn
- **THEN** the existing group ages once, the new group is painted at profile index zero, and both changes are coalesced into one frame

#### Scenario: Final fade frame completes
- **WHEN** the final visible group leaves the last fade-profile index
- **THEN** an animation frame paints its original semantic foreground before animation-only scheduling stops

## MODIFIED Requirements

### Requirement: Unicode-safe paced reveal
A transcript reveal lane SHALL reveal complete Unicode grapheme clusters and SHALL NOT expose a partial combining sequence or emoji ZWJ sequence. Semantic printable whitespace SHALL count toward the configured transcript character rate; Markdown control syntax and generated layout padding SHALL NOT count because pacing applies to rendered source text rather than raw markup or width-dependent geometry. Structural line boundaries SHALL not consume character budget.

The first admitted grapheme MAY appear immediately. Subsequent admitted graphemes in the same lane SHALL appear in one visible frame batch no faster than the configured rate permits; rates that exceed one grapheme per 16ms frame SHALL reveal a proportionally sized batch. Every grapheme in that batch SHALL share one fade group. A delayed scheduler turn SHALL NOT reveal more than one pending batch. Fade groups SHALL continue to age on independent frame deadlines after the currently admitted queue becomes empty.

#### Scenario: Combining sequence reaches the reveal boundary
- **WHEN** the next admitted rendered unit is a base character plus combining marks
- **THEN** the complete grapheme appears in one reveal step and is assigned one fade position

#### Scenario: Scheduler resumes late
- **WHEN** several nominal reveal intervals pass before the client can process the next deadline
- **THEN** that deadline reveals at most one configured frame batch per lane and schedules later batches from the actual processing time rather than displaying elapsed batches at once

#### Scenario: Admitted text reaches its current end
- **WHEN** the final currently admitted grapheme has appeared but the source may still grow
- **THEN** bounded fade-only frame steps restore the trailing groups without requiring source settlement and the lane then waits without a deadline for admission or source changes

### Requirement: Paced assistant Markdown replies
New live assistant reply blocks rendered as transcript Markdown SHALL queue their complete received text, admit a stable wrap prefix, and paint admitted content through the transcript reveal lane at no more than `message_chars_per_second`, defaulting to 120 graphemes per second. Upstream chunks MAY extend semantic content at any rate without forcing the paint cursor to catch up or moving text before the admission frontier. Snapshot replay, backward-history prepend, and already historical blocks SHALL be complete immediately rather than replaying old conversations.

#### Scenario: Assistant chunk arrives in a burst
- **WHEN** one live assistant chunk contributes many stable rendered graphemes at once
- **THEN** the transcript exposes the admitted prefix in rate-sized visible frame batches at no more than the configured message rate

#### Scenario: Upstream stream is slower than the cap
- **WHEN** a new stable assistant grapheme is admitted after the reveal lane is idle
- **THEN** it may appear immediately, its fade continues independently, and later graphemes remain subject to the configured maximum rate

#### Scenario: Network stream settles before the visual queue
- **WHEN** the assistant source stream finishes while held or admitted unrevealed graphemes remain
- **THEN** the held tail is admitted immediately, the visual queue continues at the configured character rate, and all fade groups restore their semantic foreground

#### Scenario: Historical transcript is loaded
- **WHEN** snapshot or history replay materializes an existing assistant Markdown block
- **THEN** its entire rendered content uses original semantic colors immediately and no admission, reveal, or fade deadline is started for it

### Requirement: Paced reveal for every textual Preview content kind
Whenever a Ready Preview target becomes visible, all text produced by its `PreviewContent` renderer SHALL be wrapped to the current Preview content width and revealed by terminal display row at no more than `preview_lines_per_second`, defaulting to 30 rows per second. This SHALL include links, diffs, line previews, search results, commands, paths, Markdown, reasoning, muted Markdown, plain text, structured tools and terminal output, and mutation hunks. Empty, loading, and error state labels are not Ready content and SHALL remain immediate.

One pacing unit SHALL be one non-empty wrapped display row. Structural empty rows SHALL attach to adjacent visible content and SHALL NOT consume a row-rate step by themselves. Every styled grapheme in a newly revealed row SHALL share that row's fade age while retaining its own semantic foreground, background, and modifiers.

#### Scenario: Default Preview pacing is active
- **WHEN** Ready Preview content contains multiple wrapped display rows and `preview_lines_per_second` is 30
- **THEN** the first row may appear immediately and subsequent rows appear one at a time at intervals of approximately one thirtieth of a second

#### Scenario: Long logical line wraps in Preview
- **WHEN** one Preview source line occupies several terminal rows at the current width
- **THEN** each wrapped display row consumes a separate pacing unit rather than all rows appearing as one logical-line batch

#### Scenario: Command Preview is selected
- **WHEN** a command or structured tool Preview becomes Ready
- **THEN** its header, command, metrics, and terminal-output rows appear progressively in display order with row-level fade and Preview row rate

#### Scenario: Preview target identity changes
- **WHEN** selection moves to a different Preview target, including a cached target
- **THEN** the new target starts a fresh row reveal, Preview scroll follows existing identity-change rules, and transcript reveal state is unaffected

#### Scenario: Same Preview target receives a revision
- **WHEN** a selected target is enriched or revised with a stable rendered prefix
- **THEN** already revealed common-prefix semantics remain visible, changed rows queue from the divergence, and same-target scroll is preserved

#### Scenario: Preview pacing is disabled
- **WHEN** `preview_lines_per_second` is zero
- **THEN** all wrapped rows are visible immediately with original semantic foregrounds

### Requirement: Reveal state does not alter semantic content
Reveal and admission progress SHALL be presentation-only sidecar state. The transcript store, Preview cache, original Markdown source, copy provenance, Reading View content, and event/history reconstruction SHALL retain complete semantic content independent of the current paint cursor. Resizing or theme changes SHALL rematerialize layout/styles and reapply the current semantic reveal frontier without resetting target identity, hiding already revealed semantic graphemes, or exposing queued semantic graphemes solely because row boundaries changed.

#### Scenario: Copy during transcript holdback or reveal
- **WHEN** the user copies an assistant block before its held and paced visual content completes
- **THEN** copy uses the complete original block source rather than only the admitted or visible prefix

#### Scenario: Terminal resizes during transcript reveal
- **WHEN** width changes while transcript content is held or queued
- **THEN** wrapping and its stable frontier are recomputed from complete semantic content while already revealed progress remains bounded by the reconciled common prefix

#### Scenario: Terminal resizes during Preview row reveal
- **WHEN** Preview width changes after some display rows have appeared
- **THEN** current row boundaries are recomputed while the semantic grapheme frontier remains unchanged until another row step becomes due

#### Scenario: Theme changes during fade
- **WHEN** a theme or `background_color` change is applied while transcript graphemes or Preview rows are faded
- **THEN** affected groups are recomputed from the new semantic foregrounds and background reference without changing reveal progress or fade age

#### Scenario: Plain-color mode is active
- **WHEN** `plain_color` suppresses semantic color output
- **THEN** transcript character pacing, stable admission, and Preview row pacing remain active but foreground interpolation is omitted
