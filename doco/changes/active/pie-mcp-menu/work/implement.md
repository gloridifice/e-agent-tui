# Implementation design

## 1. Baseline and goals
`auth_companion.mjs` already publishes public runtime context and exposes native resource reload. `PiAdapter` owns correlated prompt/reload admission; `e-tui` owns semantic key mapping, screen overlays and normalized events. The accepted Python modal remains an ignored prototype. Pi 1.0+ exports `getAllTools`, `getMcpServers`, command owner metadata, project trust and native MCP command text/actions; it does not export the MCP connection directory or enabled/exposure mutation API.

## 2. Overall approach
Add an embedded companion module beside the existing auth companion, registered through that companion. It uses public extension APIs, bounded Node filesystem operations and private versioned commands/status messages. It must not register the MCP command, handle `mcp_servers_change`, import Pi internals, create connections or access auth stores. The adapter recognizes the native MCP command by `sourceInfo.path == builtin:mcp`; only that owner enables frontend interception. Native subcommands remain callable directly.

A provider-neutral MCP model under `e-tui` owns modal navigation, filtering, confirmation, masked callback editing and safe snapshots. The screen renders it above the ordinary chat and below Help/approvals; terminal interception blocks background composer, selection and page actions. Dedicated semantic MCP bindings are configurable and appear in Help. Adapter data is converted to owned values before entering this model.

## 3. APIs and data model
Provider-neutral requests: catalog refresh, settings save (opaque server identity plus enabled/exposure patch and expected config revision), native login/logout/reconnect, callback reply, cancel and modal dismissal. Responses carry request identity and native session identity; stale or closed-page completions never reopen the modal. Save/reload notifications survive page dismissal as local message-area notices, not model history.

The companion sends sanitized server metadata, tool schemas/annotations, project trust, defining/override source, writable status and a hash of the config files. Read only trusted project config; retain disabled entries. Match tool namespaces to server identities rather than reverse-splitting tool names. Tool identities remain native. Use explicit bounded/truncated metadata and generic filesystem errors that do not reveal credential values.

Save validates the revision and target from a fresh read, restricts patches to enabled/exposure, preserves all unrelated JSON properties, and atomically replaces only the owning file with restrictive new-file permissions and existing file mode. Project override support follows the installed native version; malformed or ambiguous config is non-writable. Never evaluate environment/header command substitutions. Extension-only servers are inspectable but not writable.

## 4. Algorithms and rules
One correlated MCP operation per adapter. Management extension prompts are consumed before agent admission and have no streaming behavior. Mutations serialize against model/session/reload/fork operations while allowing current agent streaming to continue during file save. Save captures effective agent busy state (including fallback retry/compaction), also checks native idle state at commit, and publishes the saved result before any resource reload.

Busy saves set a pending-reload indicator and emit a local reminder; no settlement-triggered automatic reload. Idle saves hand off to the existing native reload chain. That chain rechecks `ctx.isIdle()` and refreshes catalogs only on success; completion clears pending-reload and fetches a fresh MCP view. Failure reports saved-but-not-applied and retains the pending indicator. A race to busy falls back to the reminder, not a queued model prompt.

Native MCP command status/actions are routed through correlated adapter prompts with their notifications kept inside the modal. Treat status text as display text, not a stable structured health API. Native login input uses a masked editor and cancellation replies; closing drops editor buffers and cancels outstanding native dialogs. Native URL notices remain page-local. Session replacement invalidates operation ownership. Finite adapter deadlines release the UI and explicitly report uncertain native actions rather than retrying credentials.

## 5. Fixed decisions and discretion
Approved: save while running with manual-reload reminder; save while idle with automatic native reload; modal `q` close without capturing text-editor `q`; preserve native/legacy ownership and native client. Layout, naming and internal helpers are discretionary. Blocked: none. Pi's lack of live snapshots is represented honestly through explicit refresh and native text status, not fabricated connectivity.

## 6. Verification and documentation impact
Use isolated temporary Pi agent/project roots, disabled fixture servers and no model/network/credentials for actual RPC and PTY checks. Exercise idle save/reload, injected running state and deferred-save reminder, malformed/conflicting revisions, trust, cancellation, empty state, responsive modal and editor `q`. Run existing reload/adapter and key-mapping tests, workspace formatting and Clippy; do not add or edit tests. Update current architecture and MCP sections in current specs plus the README/help quick reference; no ADR or DSH wire changes. Do not complete or archive this package.
