## Why

`/help` currently replaces the transcript with a temporary overlay, which prevents help from behaving like ordinary scrollable, selectable message content. The frontend should instead append durable-for-the-current-view Markdown help while keeping it completely local and out of the agent runtime and model context.

## What Changes

- Change the built-in `/help` action to append a non-streaming Markdown block to the transcript.
- Keep the `Ctrl+H` quick-help overlay unchanged.
- Generate the command portion of the help message from the authoritative built-in command catalog and include the currently integrated runtime commands.
- Keep help frontend-only: it emits no `AgentRequest`, bridge frame, Pi RPC command, or provider timeline event.
- Add focused command and TestBackend rendering regressions.

## Capabilities

### New Capabilities
- `local-help-message`: Defines `/help` as a local Markdown transcript message that does not enter provider state or model context.

### Modified Capabilities

None.

## Impact

The change is confined to `crates/e-tui`, primarily local command dispatch, frontend transcript insertion, help-content generation, and UI tests. DSH bridge code, the wire protocol, Pi RPC conversion, dependencies, and persisted provider sessions are unaffected.
