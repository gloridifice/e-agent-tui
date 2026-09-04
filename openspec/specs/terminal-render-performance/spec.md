# terminal-render-performance Specification

## Purpose
TBD - created by archiving change optimize-terminal-refresh-latency. Update Purpose after archive.
## Requirements
### Requirement: Event-driven interaction scheduling
The client SHALL receive keyboard, mouse, paste, and terminal resize events through an event source that directly wakes the main loop rather than waiting for a fixed animation ticker. It SHALL coalesce redundant frame requests while preserving an interactive frame deadline no later than 16ms after the current frame is available to draw, and it SHALL remain idle without periodic redraw wakeups when no content or animation is active.

#### Scenario: Wheel input arrives while the client is idle
- **WHEN** a mouse-wheel event arrives while no frame is being drawn and no network message arrives
- **THEN** the main loop handles the event immediately and requests an interactive frame without waiting for a 50ms ticker

#### Scenario: Many wheel events arrive inside one frame interval
- **WHEN** multiple wheel events arrive before the next permitted interactive frame
- **THEN** their scroll actions are applied in order and coalesced into one frame at the next interactive deadline

#### Scenario: Client has no active work
- **WHEN** there is no input, inbound content, pending frame, or running activity
- **THEN** the scheduler does not wake solely to poll terminal input or repaint an unchanged frame

#### Scenario: Resize arrives independently of model output
- **WHEN** the terminal emits a resize event without any WebSocket traffic
- **THEN** the client invalidates width-dependent layout and schedules an interactive frame directly

### Requirement: Fair processing of inbound traffic
The client SHALL process inbound bridge messages in bounded batches while preserving wire order and complete reducer semantics. A continuously ready inbound channel MUST NOT indefinitely prevent ready terminal input or an expired frame deadline from being handled.

#### Scenario: Streaming chunks and scrolling overlap
- **WHEN** assistant chunks continue to arrive while the user scrolls
- **THEN** all chunk text is folded in order and the scroll events and due frame are serviced between bounded inbound batches

#### Scenario: Snapshot burst fills the inbound queue
- **WHEN** a large snapshot or event burst leaves more messages ready than one inbound budget permits
- **THEN** the client retains the remainder in the bounded channel and continues it in subsequent scheduler turns without dropping or reordering events

#### Scenario: Batch time budget expires
- **WHEN** processing a batch reaches its configured count or time budget
- **THEN** the main loop yields to other ready event classes before attempting another inbound batch

### Requirement: Buffered and atomic terminal frame submission
The client SHALL own terminal setup and restoration through one lifecycle, SHALL buffer Crossterm output, and SHALL bracket each complete Ratatui frame transaction with synchronized-output begin/end commands when synchronized output is enabled. Ending synchronized output and restoring the terminal MUST be attempted on success, draw failure, normal exit, and panic cleanup. Terminals that ignore synchronized output MUST continue to receive a valid ordinary diff frame.

#### Scenario: Supported terminal draws a scrolling frame
- **WHEN** a scroll changes most visible transcript cells on a terminal supporting DEC private mode 2026
- **THEN** the terminal continues displaying the prior frame until the complete diff and hidden IME cursor anchor have been written and the synchronized frame is ended

#### Scenario: Terminal does not support synchronized output
- **WHEN** begin/end synchronized-output sequences are ignored by the terminal
- **THEN** the same Ratatui diff is rendered and input behavior remains functional without requiring protocol negotiation

#### Scenario: Frame drawing fails after synchronization begins
- **WHEN** the backend reports an error after the synchronized frame has begun
- **THEN** the client attempts to end synchronized output and flush before propagating the first frame error

#### Scenario: Client exits after terminal initialization
- **WHEN** the TUI exits normally or through its cleanup path
- **THEN** raw mode, alternate screen, mouse capture, bracketed paste, cursor state, and synchronized-output state are restored exactly once through the terminal owner

#### Scenario: Wide scrolling frame is emitted
- **WHEN** a frame changes thousands of cells
- **THEN** cell commands are accumulated through a buffered writer and flushed as part of the frame transaction rather than forcing an underlying stdout write per cell

### Requirement: Incremental transcript animation and content caching
The transcript cache SHALL distinguish structural invalidation, streaming-tail updates, reveal-suffix updates, width-layout invalidation, and line-count-stable message patches. Streaming chunks SHALL update only the tail when structurally possible; stable admission, paced assistant reveal, and foreground fade SHALL splice from the earliest affected transcript message rather than rebuild unrelated earlier messages; a pure animation phase change SHALL patch only active message ranges and MUST NOT rebuild unrelated transcript messages. Preview row reveal and fade SHALL patch only Preview presentation state and MUST NOT invalidate the transcript cache.

Animation scheduling SHALL compose independent activity-spinner, transcript admission, transcript content reveal, transcript fade, Preview row reveal, and Preview fade deadlines by selecting the earliest active deadline. Every positive clock SHALL respect the safe scheduler minimum, delayed turns SHALL perform bounded work without catch-up bursts, and no fixed animation ticker SHALL run when all clocks are idle.

#### Scenario: Braille indicator advances in a long transcript
- **WHEN** one activity row advances only its Braille spinner frame among hundreds of settled messages
- **THEN** the cache rerenders and patches that activity's recorded range without rebuilding settled message lines

#### Scenario: Local patch changes line count unexpectedly
- **WHEN** rerendering a dirty message produces a different number of base lines than its recorded range
- **THEN** the cache abandons the local patch and performs a safe structural rebuild

#### Scenario: Text delta extends the streaming tail
- **WHEN** a text delta appends to the existing final streaming message
- **THEN** complete semantics are folded immediately while presentation work remains bounded to the active transcript suffix

#### Scenario: Stable frontier advances
- **WHEN** a break opportunity or holdback deadline admits more rendered transcript graphemes
- **THEN** the cache splices from that assistant message's recorded start through the transcript suffix and does not rerender messages before it

#### Scenario: Foreground fade advances without content
- **WHEN** a transcript fade frame is due while no new grapheme is due and the source remains streaming
- **THEN** only the affected transcript suffix is patched and the next fade deadline remains independent of source settlement

#### Scenario: Preview row reveal advances
- **WHEN** one Preview row reveal or fade step becomes due
- **THEN** Preview is redrawn without invalidating or rebuilding transcript cache or semantic Preview cache entries

#### Scenario: Independent animation deadlines are active
- **WHEN** spinner, transcript admission/reveal/fade, and Preview reveal/fade clocks have different next deadlines
- **THEN** the event loop sleeps until the earliest deadline and advances only work whose deadline has expired

#### Scenario: Reveal callback is delayed
- **WHEN** the event loop handles a reveal, admission, or fade deadline later than requested
- **THEN** it performs at most one bounded step of each due class and bases following deadlines on the actual tick time rather than replaying elapsed intervals

#### Scenario: No animated state remains
- **WHEN** all running indicators, held tails, queued content units, and active fade groups have completed
- **THEN** animation scheduling stops and no animation-only cache invalidation occurs

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

### Requirement: Observable and enforceable frame performance
The client SHALL provide opt-in frame diagnostics and repeatable release-mode benchmark scenarios without adding per-frame logging overhead in normal operation. Diagnostics SHALL separate scheduler delay, state/update time, transcript rebuild or patch time, layout time, backend draw/flush time, complete frame time, changed cells, and emitted bytes. Reference release benchmarks SHALL cover multiple terminal sizes, a long transcript, continuous streaming, continuous scrolling, and active animation, with a P95 complete-frame target no greater than 30ms on the documented reference environment.

#### Scenario: Frame diagnostics are disabled
- **WHEN** the client runs without the diagnostics switch and without the Tracy feature active
- **THEN** it emits no performance log and does not allocate an unbounded frame-sample history

#### Scenario: Frame diagnostics are enabled
- **WHEN** diagnostics run across multiple frames
- **THEN** they emit bounded aggregate count, P50, P95, P99, and maximum values for the defined phases instead of writing one terminal-interfering line per frame

#### Scenario: Release benchmark exercises scrolling
- **WHEN** the benchmark renders repeated scroll frames at 80×40, 160×50, and 240×70 with at least 1000 transcript messages
- **THEN** it reports complete-frame latency, cache rebuild and patch counts, changed cells, and emitted bytes for each size

#### Scenario: Performance regression exceeds the project redline
- **WHEN** the documented reference release benchmark has a P95 complete-frame time above 30ms
- **THEN** the change is not considered performance-complete and the recorded phase metrics identify whether scheduler, CPU layout, cache, or terminal output work dominates

#### Scenario: Logical regression tests run on variable CI hardware
- **WHEN** ordinary unit and UI tests execute outside the reference performance environment
- **THEN** they assert bounded work and cache behavior rather than failing solely on wall-clock timing

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

