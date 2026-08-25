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
The first ordinary prompt submitted in a new-conversation draft MUST emit one typed materialization message carrying the draft mode and complete prompt text. The prompt MUST NOT enter the old session queue or be sent as an ordinary input to the old agent.

#### Scenario: Submit the first prompt
- **WHEN** the user sends `hello` from a standard-mode draft
- **THEN** the client emits one `new-input{mode:"standard",text:"hello"}` and retains the prompt until creation succeeds

#### Scenario: Bridge materializes the prompt
- **WHEN** the bridge accepts a valid `new-input`
- **THEN** it creates and attaches the requested session before following up the new agent with the prompt

#### Scenario: Creation fails
- **WHEN** session creation rejects before a new welcome is sent
- **THEN** the client remains in the draft, restores the complete prompt to the editor, and exposes the bounded bridge error

### Requirement: Real session state remains isolated behind a draft
While a draft is visible, inbound frames for the still-attached real session MUST continue updating its retained state without becoming visible in the draft transcript. A successful welcome for a different session SHALL commit the normal session switch and clear the draft.

#### Scenario: Old session event arrives
- **WHEN** an old-session event arrives while the local draft is ready
- **THEN** the retained real transcript updates but the visible draft remains empty

#### Scenario: New welcome arrives
- **WHEN** materialization produces a welcome with a session ID different from the retained ID
- **THEN** the client resets through the existing session-switch path, clears the draft, and persists only the real new ID

### Requirement: Draft commands cannot mutate the old session
Commands that are local or navigational MAY operate while a draft exists, but session-scoped model, skill, and integrated commands MUST NOT be forwarded to the old session. `/resume` SHALL remain available and a repeated `/new` SHALL remain local.

#### Scenario: Session-scoped command in a draft
- **WHEN** the user invokes `/model`, `/skill:name`, or an integrated session command before the first prompt
- **THEN** the client sends no session-scoped frame and reports that the draft must first be materialized

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

