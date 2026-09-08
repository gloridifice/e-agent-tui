## MODIFIED Requirements

### Requirement: First prompt atomically materializes the draft
The first ordinary prompt or explicit skill invocation submitted in a new-conversation draft MUST emit one typed materialization message carrying the draft mode and complete ordered text/image content. A prompt containing at least one image MUST be materializable without text. The submission MUST NOT enter the old session queue or be sent to the old agent. The client SHALL immediately display the pending user/skill card and activate its working indicator while retaining the complete submission for failure recovery.

#### Scenario: Submit the first prompt
- **WHEN** the user sends `hello` from a standard-mode draft
- **THEN** the client emits one `new-input{mode:"standard",text:"hello"}` and retains the prompt until creation succeeds

#### Scenario: Submit the first image prompt
- **WHEN** the user sends a mixed-content or image-only prompt from a draft
- **THEN** the client emits one `new-input` carrying the draft mode and every text/image part in composer order

#### Scenario: A skill opens the conversation
- **WHEN** the user sends `/skill:review` from a draft with no prior message
- **THEN** the client displays the skill immediately and requests materialization; the adapter invokes the skill only on the newly attached session
- **AND** the authoritative skill echo replaces the pending visual card without duplication and appears before Thinking

#### Scenario: Bridge materializes the prompt
- **WHEN** the bridge accepts a valid `new-input`
- **THEN** it creates and attaches the requested session before following up the new agent with the prompt

#### Scenario: Bridge materializes an image-bearing prompt
- **WHEN** the bridge accepts a valid image-bearing `new-input`
- **THEN** it creates and attaches the requested session before admitting the complete prompt through the Host session prompt API

#### Scenario: Creation fails
- **WHEN** session creation rejects before a new welcome is sent
- **THEN** the client remains in the draft, restores the complete prompt to the editor, and exposes the bounded bridge error

#### Scenario: Creation or image admission fails
- **WHEN** session creation or first-prompt image admission rejects before the draft is committed
- **THEN** the client remains in the draft, restores the complete text and image prompt to the editor, and exposes the bounded bridge error
