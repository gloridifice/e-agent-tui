## MODIFIED Requirements

### Requirement: First prompt atomically materializes the draft
The first ordinary prompt submitted in a new-conversation draft MUST emit one typed materialization message carrying the draft mode and complete ordered text/image content. A prompt containing at least one image MUST be materializable without text. The prompt MUST NOT enter the old session queue or be sent as ordinary input to the old agent.

#### Scenario: Submit the first text prompt
- **WHEN** the user sends `hello` from a standard-mode draft
- **THEN** the client emits one `new-input` carrying mode `standard` and one text content part, and retains the prompt until creation succeeds

#### Scenario: Submit the first image prompt
- **WHEN** the user sends a mixed-content or image-only prompt from a draft
- **THEN** the client emits one `new-input` carrying the draft mode and every text/image part in composer order

#### Scenario: Bridge materializes the prompt
- **WHEN** the bridge accepts a valid image-bearing `new-input`
- **THEN** it creates and attaches the requested session before admitting the complete prompt through the Host session prompt API

#### Scenario: Creation or admission fails
- **WHEN** session creation or first-prompt image admission rejects before the draft is committed
- **THEN** the client remains in the draft, restores the complete text and image prompt to the editor, and exposes the bounded bridge error
