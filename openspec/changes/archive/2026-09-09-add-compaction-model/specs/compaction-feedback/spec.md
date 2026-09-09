## MODIFIED Requirements

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
