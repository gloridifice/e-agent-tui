# local-help-message Specification

## Purpose
TBD - created by archiving change render-help-as-local-markdown. Update Purpose after archive.
## Requirements
### Requirement: `/help` appends local Markdown
The frontend SHALL handle the built-in `/help` command by appending one complete, non-streaming Markdown block to the canonical transcript instead of opening the quick-help overlay.

#### Scenario: User invokes `/help`
- **WHEN** the user submits `/help` without arguments
- **THEN** the transcript contains a new Markdown help block that participates in normal scrolling, selection, copy provenance, and Reading View

#### Scenario: Help contains available commands
- **WHEN** the frontend builds the Markdown help block
- **THEN** it derives built-in command names, descriptions, and hints from the authoritative built-in catalog and includes the current integrated command roster without duplicating built-in name collisions

### Requirement: Help remains outside agent context
The frontend MUST render `/help` without emitting an agent request, provider timeline event, bridge frame, Pi RPC command, or provider-persisted message.

#### Scenario: `/help` completes locally
- **WHEN** the local command dispatcher handles `/help`
- **THEN** its outbound agent-request collection is empty and only frontend transcript state changes

#### Scenario: A later user prompt is sent
- **WHEN** the user sends an ordinary prompt after viewing help
- **THEN** only the prompt input is sent to the agent and the local help Markdown is not included in model context

### Requirement: Quick-help overlay remains available
The existing `Ctrl+H` interaction SHALL continue to open and dismiss the quick-help overlay independently of `/help` transcript output.

#### Scenario: User presses Ctrl+H
- **WHEN** terminal routing receives the existing Ctrl+H binding
- **THEN** the quick-help overlay opens with its existing priority and dismissal behavior

