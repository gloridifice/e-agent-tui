## MODIFIED Requirements

### Requirement: Incremental transcript animation and content caching
The transcript cache SHALL distinguish structural invalidation, streaming-tail updates, reveal-suffix updates, width-layout invalidation, and line-count-stable message patches. Streaming chunks SHALL update only the tail when structurally possible; paced assistant reveal SHALL splice from the earliest affected transcript message rather than rebuild unrelated earlier messages; a pure animation phase change SHALL patch only active message ranges and MUST NOT rebuild unrelated transcript messages. Preview reveal SHALL patch only Preview presentation state and MUST NOT invalidate the transcript cache. Animation deadlines SHALL honor the configured `spinner_frame_ms` and the independently configured transcript and Preview character intervals, each subject to a safe scheduler minimum, by selecting the earliest active deadline.

#### Scenario: Breathing indicator advances in a long transcript
- **WHEN** one activity row changes only its breathing color among hundreds of settled messages
- **THEN** the cache rerenders and patches that activity's recorded range without rebuilding settled message lines

#### Scenario: Local patch changes line count unexpectedly
- **WHEN** rerendering a dirty message produces a different number of base lines than its recorded range
- **THEN** the cache abandons the local patch and performs a safe structural rebuild

#### Scenario: Text delta extends the streaming tail
- **WHEN** a text delta appends to the existing final streaming message
- **THEN** the previous cached tail is replaced without rerendering earlier messages

#### Scenario: Paced reveal extends a wrapped suffix
- **WHEN** one reveal step adds a grapheme that changes the active assistant block's rendered rows
- **THEN** the cache splices from that message's recorded start through the transcript suffix and does not rerender messages before it

#### Scenario: Preview reveal advances
- **WHEN** one Preview reveal or fade-drain step becomes due
- **THEN** the Preview is redrawn without invalidating or rebuilding transcript cache or semantic Preview cache entries

#### Scenario: Independent animation deadlines are active
- **WHEN** spinner, transcript reveal, and Preview reveal clocks have different next deadlines
- **THEN** the event loop sleeps until the earliest deadline and advances only animation work that is due

#### Scenario: Spinner cadence is configured
- **WHEN** `spinner_frame_ms` is set to a valid value and an activity remains visible
- **THEN** spinner frame requests follow that cadence rather than a hard-coded 50ms loop or either text reveal rate

#### Scenario: Reveal callback is delayed
- **WHEN** the event loop handles a reveal deadline later than requested
- **THEN** it performs bounded single-step reveal work instead of an elapsed-time catch-up burst and bases the following deadline on the actual tick time

#### Scenario: No animated state remains
- **WHEN** all running indicators, settle transitions, queued graphemes, and fade-drain transitions have completed
- **THEN** animation scheduling stops and no animation-only cache invalidation occurs
