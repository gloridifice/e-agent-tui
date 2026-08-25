## ADDED Requirements

### Requirement: Status bar shows the effective reasoning effort
The client SHALL render the current session's reasoning effort in status line 1, immediately after the cache-hit-rate entry, using the same dim style as the model and `CH` entries. The entry SHALL use the label form `Effort:<Label>` with no space after the colon.

#### Scenario: Explicit effort is displayed
- **WHEN** the current selection carries a `reasoningEffort` and the exact current model exposes that effort in its `reasoning.efforts`
- **THEN** the status bar shows `Effort:<name>` where `<name>` is the adapter-declared display name, styled identically to the model and `CH` entries

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

### Requirement: /effort command opens an effort selector
The client SHALL register a built-in `/effort` command that opens a dedicated Effort Input Page and requests the model catalog, without requiring a new wire message type.

#### Scenario: Open the selector
- **WHEN** the user executes `/effort`
- **THEN** the Effort Input Page opens and the client sends the existing `ModelGet` message

#### Scenario: Available during a deferred /new draft
- **WHEN** a client-side `/new` draft is pending
- **THEN** `/effort` remains usable and a selected effort is applied to the session materialized by the next input

### Requirement: Effort options come only from the exact current route
The Effort Input Page SHALL list only the efforts declared by the exact current provider/model route's `reasoning.efforts`, in the adapter's declared order, and SHALL never fabricate `low/medium/high` or search across providers by model id.

#### Scenario: List adapter-declared efforts
- **WHEN** the exact current model exposes `reasoning.efforts`
- **THEN** the page renders those efforts, in declared order, using their adapter display names

#### Scenario: Current model is absent from the catalog
- **WHEN** the current provider/model cannot be found in the model groups
- **THEN** the page shows an unavailable state and exposes no selectable focus target

#### Scenario: No declared efforts
- **WHEN** the exact current model exposes no `reasoning` or an empty `efforts` list
- **THEN** the page shows an empty-state message and exposes no fake focus target

### Requirement: Selecting an effort submits the full selection
Activating an effort SHALL send the complete `ModelSet` message with the current provider and model plus the chosen `reasoningEffort`, and SHALL close the page on send.

#### Scenario: Choose an effort
- **WHEN** the user focuses an effort option and presses Enter
- **THEN** the client sends `ModelSet { provider, model, reasoningEffort }` using the current provider/model and the chosen effort id, then closes the page

#### Scenario: Default marker without forcing a selection
- **WHEN** no explicit effort is set but the model declares a `defaultEffort`
- **THEN** that option is visually marked as the default but is not submitted unless the user activates an effort

### Requirement: Effort state updates through the model frame
The client SHALL derive the displayed and selectable effort solely from the `model` frame's `current` and model `reasoning` metadata, and SHALL update it when the model catalog refreshes after a model or effort change, attach, or resume.

#### Scenario: Model switch clears a stale effort
- **WHEN** a `/model` change lands and the new current selection carries no `reasoningEffort`
- **THEN** the status bar reflects the new model's default or hides the entry, and the stale previous effort is not shown

#### Scenario: Effort change refreshes the bar
- **WHEN** a `/effort` selection is accepted and the bridge re-sends the model frame
- **THEN** the status bar shows the newly selected effort
