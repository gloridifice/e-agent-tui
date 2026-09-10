## MODIFIED Requirements

### Requirement: Status bar shows the effective reasoning effort
The client SHALL render the current session's reasoning effort in status line 1, immediately after the cache-hit-rate entry, using the same normal dim style as the model and `CH` entries, with the transient foreground defined by `status-selection-feedback` after confirmed effort changes. The entry SHALL use the label form `Effort:<Label>` with no space after the colon.

#### Scenario: Explicit effort is displayed
- **WHEN** the current selection carries a `reasoningEffort` and the exact current model exposes that effort in its `reasoning.efforts`
- **THEN** the status bar shows `Effort:<name>` where `<name>` is the adapter-declared display name, using the normal dim status style outside selection feedback

#### Scenario: Adapter default is displayed
- **WHEN** the current selection has no explicit `reasoningEffort` and the exact current model exposes a `reasoning.defaultEffort`
- **THEN** the status bar shows `Effort:<name>` using the default effort's display name

#### Scenario: Provider default is displayed
- **WHEN** the exact current model exposes `reasoning` but neither the selection nor the model declares an effort
- **THEN** the status bar shows `Effort:Default`

#### Scenario: No reasoning metadata hides the entry
- **WHEN** the exact current model exposes no `reasoning` metadata
- **THEN** the status bar omits the effort entry entirely and shows no placeholder

#### Scenario: Cache-hit rate absent
- **WHEN** no cache-hit rate is available but an effort label is
- **THEN** the effort entry still renders after the model entry without a leading `CH` entry

