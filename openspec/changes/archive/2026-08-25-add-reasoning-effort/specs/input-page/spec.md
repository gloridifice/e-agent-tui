## ADDED Requirements

### Requirement: Effort page interaction
The effort Input Page SHALL render the current model's adapter-declared efforts as a single-column list governed by one focus. It SHALL show a loading state until the model catalog arrives, an unavailable state when the current route cannot be resolved or exposes no efforts, and a key-hint footer otherwise.

#### Scenario: Choose an effort
- **WHEN** the user focuses an effort option and presses Enter
- **THEN** the client sends `ModelSet` with the current provider/model and the chosen `reasoningEffort`, and closes the effort Input Page

#### Scenario: No selectable efforts
- **WHEN** the current route exposes no `reasoning.efforts` or cannot be found in the catalog
- **THEN** the page shows an empty/unavailable message and exposes no fake focus target

#### Scenario: Wait for the catalog
- **WHEN** the effort page is open and its requested model catalog has not arrived
- **THEN** the page shows a loading state rather than reporting an empty catalog as a completed result

## MODIFIED Requirements

### Requirement: Existing transport compatibility
The Input Page system SHALL use the existing login, proxy, model, and configuration messages, and the effort page SHALL reuse the existing `model-get`/`model-set` wire messages. This change SHALL NOT introduce a new wire message type, but it MAY require the wire protocol-version bump to v6 so peers agree on the new optional `model-set.reasoningEffort` and model `reasoning` fields.

#### Scenario: Send page action
- **WHEN** an Input Page performs a login, proxy, model, or effort operation
- **THEN** the client emits the existing corresponding `ClientMessage` shape (`ModelSet` may carry the optional `reasoningEffort`) without introducing a new wire message type
