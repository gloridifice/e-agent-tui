## ADDED Requirements

### Requirement: Skill commands retain a trailing user prompt
Both DSH and Pi adapters SHALL accept `/skill:<name> <text>` and `/skill <name> <text>`, inject the resolved skill instructions first, then enqueue the trailing text as a separate ordinary user message on the same session. Separator whitespace SHALL be removed, while the remaining text, including internal newlines and trailing whitespace, SHALL be preserved. A command with no non-whitespace text SHALL inject only the skill. Pi SHALL retain native skill expansion and wait for skill prompt admission before sending the trailing text through its ordinary prompt path.

#### Scenario: Skill and text submitted together
- **WHEN** the user submits `/skill:review Check this code`
- **THEN** the adapter enqueues the review skill invocation followed by the user message `Check this code`, in that order

#### Scenario: Skill opens a draft with text
- **WHEN** a deferred new session is materialized with `/skill:review Check this code`
- **THEN** both messages are enqueued only on the newly attached session, with the skill first

#### Scenario: No trailing prompt
- **WHEN** a skill command contains only its name and optional whitespace
- **THEN** only the skill invocation is enqueued

#### Scenario: Skill cannot be admitted
- **WHEN** skill lookup/admission fails or the session becomes stale before admission
- **THEN** neither the skill nor the trailing user prompt is enqueued
