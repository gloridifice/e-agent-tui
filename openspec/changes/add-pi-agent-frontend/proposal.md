## Why

The kernel-neutral `e-tui` frontend currently has only a DSH executable adapter, so Pi users cannot use the same ratatui presentation with Pi's native models, tools, resources, extensions, and sessions. Pi 0.84.x now exposes a documented cross-language RPC mode that provides a practical integration boundary without reimplementing its agent loop or binding Rust to unstable SDK internals.

## What Changes

- Add workspace package `crates/e-pi` with executable artifact `pie`.
- Launch the installed Pi CLI in `--mode rpc` and communicate through strict LF-delimited JSONL over child stdio.
- Normalize Pi session, message, streaming, tool, retry, compaction, model, command, and Extension UI records into `e-tui`'s provider-neutral contracts.
- Translate frontend requests for prompting, aborting, deferred new sessions, session switching, model/thinking selection, compaction, and extension dialog answers into Pi RPC commands.
- Reuse Pi's native configuration, authentication, resource discovery, project-trust policy, extensions, and session storage rather than introducing parallel state.
- Provide current-project session discovery for the existing Resume page while treating Pi session files as read-only metadata at that boundary.
- Add actionable startup/crash diagnostics and child-process cleanup.
- Keep first-release authentication management external: users authenticate with native `pi /login`; `pie` consumes the resulting native credentials.

## Capabilities

### New Capabilities
- `pi-agent-frontend`: Covers `pie` startup, Pi RPC transport, normalized event/request behavior, native-state compatibility, session/model/command integration, and supported Extension UI behavior.

### Modified Capabilities
- `kernel-neutral-agent-tui`: Requires a second adapter executable to consume the existing normalized frontend contracts without adding Pi protocol names or process I/O to `e-tui`.

## Impact

- Adds `crates/e-pi`, a new workspace member and `pie` binary.
- Adds a Pi-specific JSONL protocol/process adapter and focused compatibility tests.
- Extends provider-neutral frontend interaction contracts only where Pi RPC exposes behavior not currently represented.
- Initially reuses the existing executable-side frontend runtime infrastructure from `e-dsh` because the reducer/controller migration into a fully shared composition crate is incomplete; Pi wire types remain isolated in `e-pi`, and no DSH service or bridge is used by `pie`.
- Requires an executable `pi` 0.84.x-compatible CLI on `PATH`; Node and Pi packaging remain external in this source-install phase.
