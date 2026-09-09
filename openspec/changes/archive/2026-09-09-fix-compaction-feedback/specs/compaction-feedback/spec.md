## ADDED Requirements

### Requirement: Successful compaction feedback
A successful compaction SHALL replace its running activity label with `Compacting complete`. A failed compaction SHALL retain failure presentation and its error rather than claim completion. Settlement arriving before its start through history paging SHALL produce the same completed label.

#### Scenario: Successful settlement
- **WHEN** compaction completes successfully
- **THEN** its activity displays `Compacting complete` instead of the running label

#### Scenario: Failure
- **WHEN** compaction fails
- **THEN** its activity displays failure and the reported error without a success label

### Requirement: Unknown post-compaction context usage
After successful compaction, the status bar SHALL display `?%` in place of the context percentage until a subsequent nonzero assistant usage sample is received. Empty or absent usage SHALL NOT restore an estimate. Cumulative token/cache statistics SHALL remain intact. Historical prepend SHALL NOT replace the current known/unknown status; attachment SHALL reset it before chronological replay.

#### Scenario: Wait for real usage
- **WHEN** successful compaction is followed by absent or zero assistant usage
- **THEN** the status bar continues to show `?%` with the existing context window

#### Scenario: New sample
- **WHEN** a subsequent nonzero assistant usage sample arrives
- **THEN** the percentage is calculated from that sample

#### Scenario: Older history
- **WHEN** older history is prepended while context usage is unknown
- **THEN** the current percentage remains unknown
