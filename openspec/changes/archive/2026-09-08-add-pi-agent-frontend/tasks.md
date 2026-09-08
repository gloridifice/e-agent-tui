## 1. Workspace and boundaries

- [x] 1.1 Add `crates/e-pi` as a workspace package with library modules and `pie` binary target
- [x] 1.2 Add Pi-specific architecture checks that keep RPC/process names out of `e-tui` and DSH bridge modules out of `e-pi`
- [x] 1.3 Add `pie`-scoped frontend config/theme persistence while reusing `e-tui` value schemas

## 2. Pi RPC protocol and process

- [x] 2.1 Define typed outbound Pi RPC commands and inbound response/event/Extension UI DTO parsing
- [x] 2.2 Implement strict LF-only bounded JSONL framing with CRLF tolerance and Unicode-separator coverage
- [x] 2.3 Implement Pi CLI discovery/spawn, stderr diagnostics, bounded channels, stdin writes, and bounded shutdown/reaping
- [x] 2.4 Add CLI parsing for session selection and one-run project-trust overrides

## 3. Request and event adapter

- [x] 3.1 Translate frontend prompt, abort, command, model/thinking, attach, deferred-new, and heartbeat requests into Pi RPC operations
- [x] 3.2 Normalize startup state/messages/commands/models into authoritative `AgentEvent` snapshots and catalogs
- [x] 3.3 Normalize live assistant text/reasoning, lifecycle, usage, retry, compaction, and errors into timeline/session events
- [x] 3.4 Normalize known Pi tools into provider-neutral capabilities/previews and unknown extension tools into bounded generic surfaces
- [x] 3.5 Chain deferred `new_session` and first prompt safely, restoring the retained prompt on failure

## 4. Native sessions and Extension UI

- [x] 4.1 Implement bounded read-only native Pi session metadata discovery for the current project
- [x] 4.2 Map Resume selection to Pi `switch_session` and refresh authoritative state after replacement
- [x] 4.3 Map Extension UI select/confirm/input/editor requests and responses through existing ratatui interactions
- [x] 4.4 Support Extension UI notify and visible composer text updates; degrade unsupported RPC UI methods explicitly

## 5. `pie` composition

- [x] 5.1 Compose Pi process input, normalized events, terminal events, deadlines, rendering, and effect execution in the event-driven main loop
- [x] 5.2 Preserve bounded inbound work, streaming redraw behavior, terminal restoration, child cleanup, and actionable fatal diagnostics
- [x] 5.3 Run an installed-Pi smoke test covering startup queries, one prompt, streaming completion, and clean shutdown

## 6. Verification and public workflow

- [x] 6.1 Add focused protocol, adapter, session-index, request-chain, and Extension UI regression tests
- [x] 6.2 Update current client architecture documentation for the `e-pi` boundary and add concise `pie` source-install/run guidance
- [x] 6.3 Run `cargo fmt --all`, targeted `e-pi` tests, workspace clippy, and relevant architecture tests
