# compaction-feedback Specification

## Purpose
Show successful compaction settlement clearly and avoid presenting stale context usage before the next assistant sample.

## Requirements

### Requirement: Successful compaction feedback
Running compaction SHALL display `compacting with <model_name>` when the actual model is known, otherwise `compacting`. A successful compaction SHALL replace its running activity label with `compacting complete with <model_name>`, or `compacting complete` when the model is unavailable. A failed compaction SHALL retain failure presentation and its error rather than claim completion. Settlement arriving before its start through history paging SHALL produce the same completed label. Historical events without model metadata SHALL not borrow the currently configured override.

#### Scenario: Successful settlement
- **WHEN** compaction completes successfully with a known model
- **THEN** its activity displays `compacting complete with <model_name>` instead of the running label

#### Scenario: Failure
- **WHEN** compaction fails
- **THEN** its activity displays failure and the reported error without a success label

#### Scenario: Legacy history
- **WHEN** a historical compaction lacks model metadata
- **THEN** its successful label is `compacting complete`

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
