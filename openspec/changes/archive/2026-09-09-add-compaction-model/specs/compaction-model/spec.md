## ADDED Requirements

### Requirement: Select a compaction model
`/compact set-model` SHALL open the existing model picker for compaction selection rather than conversation selection. `/compact set-model <model_id>` SHALL accept a unique bare id or canonical provider/model id from the catalog. Invalid or ambiguous selections SHALL report an error without changing the selection. `/compact unset-model` SHALL clear the override. Overrides SHALL be runtime-only: per session in DSH and per running Pi adapter in Pi.

#### Scenario: Picker selection
- **WHEN** a model is selected from `/compact set-model`
- **THEN** future applicable compactions use it without changing the conversation model

#### Scenario: Clear selection
- **WHEN** `/compact unset-model` succeeds
- **THEN** subsequent compactions use the backend's ordinary model policy

### Requirement: Backend-specific compaction routing
DSH SHALL use the selected model for manual and automatic compaction summary calls only, preserving conversation routing. Pi SHALL use it only for manual `/compact`; automatic compaction SHALL retain native behavior. A Pi manual compaction SHALL wait for successful model selection, restore the prior model and thinking level after success, failure, or cancellation, and hold dependent prompts/model/session changes until restoration completes. A selection failure SHALL not run compaction. Restoration failure SHALL be surfaced and SHALL not silently release dependent work under the wrong model.

#### Scenario: DSH automatic compaction
- **WHEN** DSH automatically compacts with an override configured
- **THEN** its summary request uses the override while ordinary requests retain their original route

#### Scenario: Pi manual compaction failure
- **WHEN** manual compaction fails after switching models
- **THEN** Pi restores the original model and thinking level before releasing dependent requests and reports the error

#### Scenario: Pi automatic compaction
- **WHEN** Pi automatically compacts with an override configured
- **THEN** it uses the native conversation model without a temporary selection
