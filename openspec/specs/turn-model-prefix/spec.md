# turn-model-prefix Specification

## Purpose
Allow a prompt to use a saved model mark for one complete conversation turn, with visible temporary selection and automatic restoration.

## Requirements

### Requirement: Leading marked-model prompt
A leading `//<lowercase ASCII letter>` followed by whitespace or end of input SHALL resolve the saved mark's exact provider/model route. Sending SHALL remove the prefix and its separating whitespace, preserve the remaining text and images, and wait for successful model selection before admitting the prompt. The original provider/model/reasoning-effort selection SHALL be retained for restoration. Unknown or unavailable marks and empty prefixed prompts SHALL NOT send or change models and SHALL remain recoverable. A prefix is a prompt modifier, not a local slash command.

#### Scenario: Send with a marked model
- **WHEN** `i` identifies `gpt-5.6-luna` and the user sends `//i commit`
- **THEN** the selected marked model receives only `commit` after selection is confirmed

#### Scenario: Invalid or empty modifier
- **WHEN** the user submits an unavailable mark or only `//i`
- **THEN** no prompt or model change is sent and the input remains recoverable

#### Scenario: Opening a deferred conversation
- **WHEN** a marked prompt is sent from a deferred `/new` draft
- **THEN** the new session receives the stripped opening prompt with the temporary model and retains the prior selection for restoration

### Requirement: Temporary model follows the complete turn
The temporary model SHALL remain selected throughout the current conversation turn, including tool continuations and unprefixed ASAP steering. Completion, interruption, or failure SHALL restore the original selection before a subsequent unprefixed prompt is dispatched. After-turn prompts SHALL wait for restoration. Queued marked prompts SHALL retain their own model choice, and another marked prompt SHALL NOT replace the original restoration target with a temporary model. Session attachment unrelated to draft materialization SHALL discard old temporary state rather than apply it to the newly attached session.

#### Scenario: Steering keeps the temporary model
- **WHEN** an unprefixed ASAP message is inserted during a temporary-model turn
- **THEN** it is delivered as steering without restoring the original model

#### Scenario: After-turn returns to the original model
- **WHEN** an unprefixed after-turn message is queued during a temporary-model turn
- **THEN** the original provider/model/effort is restored after the complete turn settles and before that message is sent

#### Scenario: Selection fails
- **WHEN** selecting a temporary model fails
- **THEN** the dependent prompt is not sent under another model, remains recoverable, and the error is displayed

#### Scenario: Restoration fails
- **WHEN** restoring the original model fails
- **THEN** the failure is visible and subsequent prompts are not silently sent under the temporary model

### Requirement: Temporary model presentation
The composer SHALL show the resolved model name immediately after a recognized leading mark in the theme's Umber-equivalent tone, while the typed prefix keeps ordinary input styling. The name SHALL be presentation-only: it SHALL NOT enter the editable buffer, history payload, or submitted prompt. The status-bar model name SHALL be italic while a temporary model is selected and return to ordinary styling once restoration is confirmed.

#### Scenario: Preview a mark
- **WHEN** the user types `//i` for `gpt-5.6-luna`
- **THEN** the composer displays `//i gpt-5.6-luna`, with only the model-name preview in Umber and no model-name insertion into the buffer

#### Scenario: Temporary status
- **WHEN** the marked route is active and later the original selection is restored
- **THEN** the model name is italic only during the temporary selection
