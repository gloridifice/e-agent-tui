## ADDED Requirements

### Requirement: Typed tool Preview normalization boundary
The DSH adapter SHALL normalize tool-call arguments and supported result presentation metadata into bounded provider-neutral Preview facts. `e-tui` reducers and renderers MUST NOT inspect raw DSH tool names, raw argument JSON fields, or opaque result metadata. Unsupported, malformed, or bridge-trimmed optional presentation metadata SHALL fail soft to a typed path, JSON, or no-specialized-preview fallback.

#### Scenario: Known call schema is normalized
- **WHEN** DSH emits a read, view, create, filesystem-search, command, bash, pwsh, edit, replace, or insert call with valid arguments
- **THEN** the adapter emits the corresponding typed Preview seed while retaining existing capability and transcript projection facts

#### Scenario: DSH edit result carries diff metadata
- **WHEN** a tool result contains valid bounded `meta.diffs` entries with path, old text, and new text
- **THEN** the protocol/adapter boundary emits ordered typed mutation hunks without computing a new diff

#### Scenario: Metadata is malformed or trimmed
- **WHEN** result metadata is absent, malformed, or replaced by the bridge's bounded metadata fallback
- **THEN** the event still settles its activity and Preview degrades without exposing arbitrary JSON to `e-tui`

#### Scenario: Unknown tool arguments are normalized
- **WHEN** DSH emits an unsupported tool name
- **THEN** the adapter emits the original display name and bounded normalized JSON source rather than requiring renderer-side DSH schema inspection

### Requirement: Tool Preview facts survive event reconstruction
Typed tool Preview seeds, result output, metrics inputs, mutation metadata, and truncation state SHALL be derived consistently from live, snapshot, and history events. Cross-page result-before-call ordering SHALL preserve enough bounded typed state to finalize the same Preview when the call is later observed.

#### Scenario: Live and replayed command agree
- **WHEN** the same command call/result pair is reduced live and from a snapshot
- **THEN** both paths produce equivalent Preview name, command text, final metrics, truncation qualification, and secondary output

#### Scenario: Older page supplies a missing call
- **WHEN** a result and its typed Preview facts are staged from a newer page and the matching call arrives in an older page
- **THEN** the adapter/projector correlates them by call ID without reparsing raw event JSON in the renderer
