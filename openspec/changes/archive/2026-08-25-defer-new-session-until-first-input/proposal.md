## Why

`/new` currently creates and persists a DSH session immediately, so conversations that never receive a first prompt still pollute `/resume` history. A new conversation should exist only as a client draft until the user performs the first session-engaging action.

## What Changes

- Make `/new [mode]` enter a client-only “新对话” draft without sending `/new` to the bridge or inventing/persisting a session ID.
- Materialize the draft atomically on its first ordinary prompt through a typed `new-input` wire message carrying the chosen mode and text.
- Preserve the attached real session behind the draft until materialization succeeds; creation failure keeps the draft and prompt recoverable.
- Exclude sessions with no `turn/start` from `/resume`, including old setup-only records and the startup-created blank session.
- Treat `/resume` and repeated `/new` as local draft transitions; keep unsafe session-scoped actions explicit while a draft is pending.
- Display the client-only draft title as `新对话` without writing a synthetic `session/title` event.

## Capabilities

### New Capabilities
- `deferred-new-conversation`: Client draft lifecycle, first-input materialization, blank-history visibility, and failure/retry behavior.

### Modified Capabilities
- `canonical-wire-schema`: Add the typed, bounded `new-input` client message and synchronize all generated protocol artifacts.

## Impact

- Rust client runtime/controller, command routing, UI page metadata, and scripted tests.
- Node bridge dispatcher/session creation and session-list filtering.
- `bridge/protocol-contract.json`, generated protocol documentation/fixtures/package metadata, and both Rust/Node conformance tests.
- Project design and agent-maintenance documentation; no persistence deletion API or raw session-file mutation is introduced.
