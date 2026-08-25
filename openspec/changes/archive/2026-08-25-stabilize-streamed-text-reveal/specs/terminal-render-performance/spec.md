## MODIFIED Requirements

### Requirement: Incremental transcript animation and content caching
The transcript cache SHALL distinguish structural invalidation, streaming-tail updates, reveal-suffix updates, width-layout invalidation, and line-count-stable message patches. Streaming chunks SHALL update only the tail when structurally possible; stable admission, paced assistant reveal, and foreground fade SHALL splice from the earliest affected transcript message rather than rebuild unrelated earlier messages; a pure animation phase change SHALL patch only active message ranges and MUST NOT rebuild unrelated transcript messages. Preview row reveal and fade SHALL patch only Preview presentation state and MUST NOT invalidate the transcript cache.

Animation scheduling SHALL compose independent spinner/settle, transcript admission, transcript content reveal, transcript fade, Preview row reveal, and Preview fade deadlines by selecting the earliest active deadline. Every positive clock SHALL respect the safe scheduler minimum, delayed turns SHALL perform bounded work without catch-up bursts, and no fixed animation ticker SHALL run when all clocks are idle.

#### Scenario: Breathing indicator advances in a long transcript
- **WHEN** one activity row changes only its breathing color among hundreds of settled messages
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
- **WHEN** all running indicators, settle transitions, held tails, queued content units, and active fade groups have completed
- **THEN** animation scheduling stops and no animation-only cache invalidation occurs
