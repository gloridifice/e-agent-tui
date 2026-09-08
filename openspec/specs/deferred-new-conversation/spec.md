# deferred-new-conversation Specification

## Purpose
TBD - created by archiving change defer-new-session-until-first-input. Update Purpose after archive.
## Requirements
### Requirement: `/new` creates only a client draft
The client MUST handle `/new [mode]` without sending a session-creation command, changing the persisted real session ID, or discarding the attached session’s transcript. The visible draft SHALL have an empty transcript and the display-only title `新对话`.

#### Scenario: Open a new draft
- **WHEN** the user executes `/new code` from an idle attached session
- **THEN** the client displays an empty `新对话` in code mode and emits no bridge message

#### Scenario: Replace an unused draft
- **WHEN** the user executes another `/new minimal` before materialization
- **THEN** the existing draft is replaced locally and no server session is created

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

### Requirement: Real session state remains isolated behind a draft
While a draft is visible, inbound frames for the still-attached real session MUST continue updating its retained state without becoming visible in the draft transcript. A successful welcome for a different session SHALL commit the normal session switch. During materialization, the pending draft card SHALL remain visible until the opening user/skill echo arrives or admission fails.

#### Scenario: Old session event arrives
- **WHEN** an old-session event arrives while the local draft is ready
- **THEN** the retained real transcript updates but the visible draft remains empty

#### Scenario: New welcome arrives
- **WHEN** materialization produces a welcome with a session ID different from the retained ID
- **THEN** the client resets through the existing session-switch path and persists only the real new ID, retaining pending submission feedback until its authoritative echo arrives

### Requirement: Draft commands cannot mutate the old session
Commands that are local or navigational MAY operate while a draft exists. `/model` and `/effort` SHALL remain usable, and their selected provider/model/effort SHALL apply to the materialized session before its first submission. A skill invocation SHALL materialize the draft rather than execute on the retained old session. Other integrated commands MUST NOT be forwarded to the old session. `/resume` SHALL remain available and a repeated `/new` SHALL remain local.

#### Scenario: Integrated command in a draft
- **WHEN** the user invokes an integrated session command before the first prompt
- **THEN** the client sends no session-scoped frame and reports that the draft must first be materialized

#### Scenario: Model and effort selection before materialization
- **WHEN** the user changes model or effort and then immediately submits the first prompt or skill
- **THEN** selection settles before admission, and native session creation does not revert either selection to the previous/default value

#### Scenario: Resume from a draft
- **WHEN** the user opens `/resume` from a draft and attaches a listed session
- **THEN** the real welcome abandons the draft and displays the selected session

### Requirement: Blank sessions are absent from resume history
The bridge MUST exclude sessions with no `turn/start` from every partial and final `/resume` session frame. Eligibility MUST be determined before applying the list limit, and an operational classification failure MUST fail open rather than hide a potentially real conversation.

#### Scenario: Setup-only live session
- **WHEN** a live session contains permission, sandbox, and approval setup events but no `turn/start`
- **THEN** it is absent from `/resume`

#### Scenario: Existing cold blank session
- **WHEN** a persisted setup-only session predates the client draft behavior
- **THEN** cold projection or log classification excludes it without deleting its artifact

#### Scenario: First turn starts
- **WHEN** a previously blank session records `turn/start`
- **THEN** the next `/resume` listing includes it and performs normal title enrichment

#### Scenario: Blank rows exceed the list limit
- **WHEN** newer blank sessions precede older nonblank sessions
- **THEN** blanks consume no part of the 200-row visible limit

