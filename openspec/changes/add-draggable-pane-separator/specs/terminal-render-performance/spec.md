## ADDED Requirements

### Requirement: Pane separator drag rendering remains bounded
A captured pane separator drag SHALL update only transient resize geometry and SHALL render only bounded placeholder presentation until release. Drag reports MUST NOT change committed pane width, invalidate or rebuild transcript layout, materialize real Preview rows, rebuild Reading geometry, or persist Config. Release SHALL commit the final percentage once and permit the existing width-aware caches to rematerialize real content once. Normal and placeholder frames SHALL use the same one-column Main and split Preview insets, including the one-column gap after the separator.

#### Scenario: Multiple drag reports arrive before a frame
- **WHEN** several separator drag coordinates arrive inside one interactive frame interval
- **THEN** the reducer applies them in order and the scheduler may coalesce their placeholder presentation into the next interactive frame

#### Scenario: Placeholder frame is rendered
- **WHEN** pane resize is active and an interactive frame is due
- **THEN** transcript and Preview rebuild, patch, and materialized-row work counters remain zero while the pending Bark boxes and separator feedback are drawn

#### Scenario: Content arrives during resize
- **WHEN** bridge events or reveal deadlines update semantic state while the separator is captured
- **THEN** those updates continue reducing normally but drag frames still avoid real pane rendering and the restored frame shows current content

#### Scenario: Separator is released
- **WHEN** the final pending percentage differs from the committed percentage
- **THEN** the real message and Preview layouts rematerialize for the new width only after release and Config persistence is requested once

#### Scenario: Separator drag is cancelled
- **WHEN** focus loss or terminal resize cancels a pending gesture
- **THEN** no pending percentage is persisted and the next normal frame reuses the previously committed width

#### Scenario: Client becomes idle after resize
- **WHEN** the release frame and Config effect have completed and no other work is active
- **THEN** the event loop has no separator-specific polling or animation wakeup
