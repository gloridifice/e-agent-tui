<!-- doco:managed template=v1 -->
# Implementation design

## 1. Baseline and goals

Scope and acceptance belong to [the proposal](../proposal.md). Repository
baseline: commit `8249001`; the worktree was clean before package creation.
No implementation changes from the preceding research exist.

### Repository entry points

| Entry point | Existing behavior / intended responsibility |
| --- | --- |
| [crates/e-pi/src/main.rs](../../../../../crates/e-pi/src/main.rs) | Owns CLI parsing, request routing and the Pi runner; currently no login subcommand. |
| [crates/e-pi/src/process.rs](../../../../../crates/e-pi/src/process.rs) | Launches official Pi RPC, resolves Windows launchers, and owns bounded stdio and shutdown. |
| [protocol.rs](../../../../../crates/e-pi/src/protocol.rs), [framing.rs](../../../../../crates/e-pi/src/framing.rs) in e-pi | Own Pi DTOs and bounded LF-delimited framing; authentication must not masquerade as ordinary prompts. |
| [crates/e-pi/src/adapter/request.rs](../../../../../crates/e-pi/src/adapter/request.rs) | Rejects `LoginGet`, `LoginSetApiKey`, and proxy mutations. |
| [crates/e-pi/src/adapter/extension.rs](../../../../../crates/e-pi/src/adapter/extension.rs) | Projects generic extension UI into questions/approvals; notifications can enter the timeline. This is not a secret-aware authentication channel. |
| [action.rs](../../../../../crates/e-tui/src/action.rs), [agent/mod.rs](../../../../../crates/e-tui/src/agent/mod.rs) in e-tui | Own provider-neutral requests/events; `CredentialProvider` currently models API-key status only. |
| [login.rs](../../../../../crates/e-tui/src/login.rs), [input_page.rs](../../../../../crates/e-tui/src/input_page.rs), [ui/pages/login.rs](../../../../../crates/e-tui/src/ui/pages/login.rs) in e-tui | Own the existing API-key/proxy state machine, focus, and presentation. |
| [command_catalog.rs](../../../../../crates/e-tui/src/command_catalog.rs), [runtime/command.rs](../../../../../crates/e-tui/src/runtime/command.rs), [runtime/controller/agent.rs](../../../../../crates/e-tui/src/runtime/controller/agent.rs) in e-tui | Own commands and page reduction; **/login** currently rejects arguments. |
| [crates/e-pi/Cargo.toml](../../../../../crates/e-pi/Cargo.toml) | Published package currently includes the source tree, README, and license only; any shipped helper assets need explicit packaging. |

Relevant tests include the inline login and focus tests, the secret-exclusion
screen-copy regression in [selection_tests.rs](../../../../../crates/e-tui/src/runtime/controller/selection_tests.rs), inline Pi
process/adapter tests, and [e-pi architecture tests](../../../../../crates/e-pi/tests/architecture.rs).

### Native Pi baseline

Research used installed **@earendil-works/pi-coding-agent** 0.85.1 and its bundled
Pi AI package. These are version-specific observations, not a support range.
Recheck the selected runtime during validation; do not use machine-specific
installation paths in production.

Read native **docs/providers.md**, **docs/rpc.md**, **docs/sdk.md**,
**docs/custom-provider.md**, and their relevant examples before integration.
The following paths belong to the installed packages, not this repository:

- **dist/core/model-runtime.d.ts** and **.js**: `getProviders`, `login`, `logout`,
  `listCredentials`, and `refresh`; credential synchronization failure semantics.
- **dist/core/model-registry.d.ts**: the public extension facade has no
  `login`/`logout` methods and keeps its runtime private.
- **dist/modes/interactive/interactive-mode.js**: native method/provider selection,
  `AuthInteraction` mapping, and post-authentication synchronization.
- **dist/modes/rpc/rpc-mode.js**: no authentication commands;
  `get_available_models` returns `getAvailableSnapshot()`.
- **dist/core/agent-session-services.js**: registers providers contributed by
  resource-loader factories; this alone does not prove parity with providers
  registered later in a running extension lifecycle.
- Pi AI **dist/auth/types.d.ts**: text, secret, select, and manual-code prompts;
  info, authorization-URL, device-code, and progress events; whole-flow and
  per-prompt abort signals.

Read-only enumeration found seven built-in OAuth providers: Anthropic, GitHub
Copilot, Kimi Code, OpenAI Codex, OpenRouter, Radius, and xAI. API-key methods can
also include configuration choices and multiple fields, notably Cloudflare and
Google Vertex. Built-in extensions such as llama.cpp add further setup flows,
but Pi does not export its built-in extension factory set through the public SDK.
This change therefore supports Pi AI built-ins and separately loaded user
extension factories; non-exported built-in-extension setup and later runtime
registrations are explicit limitations, not a maintained provider registry.

## 2. Overall approach

### Approved integration

Use an adapter-owned, lazily started SDK authentication helper while leaving the
existing official RPC conversation child in place. A bundled companion extension
runs inside that child and uses only public extension APIs to report runtime
context and refresh its owning `ModelRegistry` after native credential changes.
The helper independently loads Pi AI built-ins, `models.json`, and eligible user
extension factories under the reported cwd, agent directory, and trust decision.

The validation must establish all of the following:

1. Resolve the SDK from the same selected Pi installation as the conversation
   process, including `--pi`, `PIE_PI_COMMAND`, wrappers, and Windows shims. Never
   silently use an unrelated globally installed SDK or download one on demand.
2. Reuse effective agent directory, cwd, configuration, and trust decisions.
   Authentication must not load a rejected project's extensions or change trust.
3. Enumerate eligible providers including model-less and factory-time user
   extension providers. Later runtime registrations and non-exported Pi built-in
   extension factories are out of scope. The helper loads eligible factories once;
   documentation warns extension authors that factory side effects may therefore
   run in both isolated processes.
4. Deliver secret-aware, cancellable interaction over a bounded private channel
   with explicit flow/prompt correlation, not model prompts or generic timeline
   notifications. A missing extension command must never fall through to an LLM.
5. Invoke a supported refresh path in the original Pi process after credential
   changes. Demonstrate that an initially unavailable provider becomes usable
   without process restart or conversation replacement.
6. Ship helper/companion assets with `cargo install e-pi`, and negotiate support
   without requiring users to install a global extension manually.

The companion extension is limited to context reporting and non-network model
refresh. It must not receive credentials or implement provider login. Do not
access `ctx.modelRegistry` private fields or assume `ctx.ui.custom()` works in RPC
mode. SDK-host replacement or upstream RPC changes require a revised architecture
decision, not an automatic fallback.

[Task 1.1 validation](validation-report.md) proved that an isolated helper can
load factory-time providers, call native multi-step login, persist into an
isolated native auth store, and ask the original RPC process to refresh through
public `ctx.modelRegistry.refresh()`. It also established the now-approved scope
boundary: later runtime registrations are not mirrored.

### Fixed ownership

- Pi's native provider implementation performs authorization, callback serving,
  polling, token exchange/refresh, and credential mutation.
- `e-pi` owns bridge assets, runtime resolution, protocol normalization, external
  effects, deadlines, and cleanup. It does not depend on `e-dsh` or reuse the
  DSH [bridge package](../../../../../bridge/) for Pi authentication.
- `e-tui` owns a capability-driven authentication page and neutral request/event
  values. It owns no Pi imports, credential paths, networking, or file writes.
- All effects execute after frontend locks are released. Credential operations
  never share the prompt/ASAP/follow-up admission path.

Approved flow:

```text
/login -> neutral page request -> e-pi authentication control
                                  -> Pi SDK native provider flow
prompt/notification <- normalized events <- native AuthInteraction
user answer/cancel -> correlated control -> native flow
credential commit -> live Pi synchronization -> fresh catalog/state -> page
```

## 3. APIs and data model

### Existing native APIs

The SDK exposes `login(providerId, type, interaction): Promise<Credential>` and
`logout(providerId, options): Promise<void>`. The returned credential and any
credential attached to `CredentialSynchronizationError` remain inside the native
integration; they are not frontend result payloads.

### Planned frontend semantics

The following defines semantic values; private Rust/JavaScript names may vary.
The helper protocol is version 1, strict LF-delimited JSONL, with a 256 KiB
record limit and channels capped at 64 records.

- Catalog request/result: operation identity, runtime generation, providers with
  stable IDs/display labels, method IDs/labels and interactive-vs-guidance
  capability, safe configured-source status, and stored-credential removability.
  Configured presence is not proof of a remotely valid credential.
- Start: catalog generation, provider ID, and method ID. Labels are presentation;
  selection replies return stable option IDs, never label-based lookup.
- Prompt: flow ID, prompt ID, input kind, message, placeholder, or selectable
  ID/label/description options. Answers preserve empty strings where native
  setup permits them; empty input is not cancellation.
- Notification: flow ID and bounded info/link, authorization URL, device code
  (including native interval/expiry when present), or transient progress.
- Reply/cancel: flow and prompt identity plus either an answer or cancellation.
  A separate prompt-withdrawal event handles an out-of-band callback winning the
  race against manual input without cancelling the whole login.
- Outcome: credential mutation state (`not_committed`, `committed`, or
  `unknown`), live-runtime synchronization status, remote-refresh warning, and
  safe actionable diagnostic. Removal and login use the same distinction.

Frontend state proceeds through catalog loading, provider/method selection,
active interaction, synchronization, and terminal outcome. The existing DSH
API-key/proxy branch remains available through capabilities; Pi does not display
unsupported proxy controls. **/logout** lists only removable stored credentials
and requires explicit user selection/confirmation.

### Lifetime, compatibility, and sensitive data

- One active authentication flow per frontend. Reject another start while the
  first is active rather than racing credential mutations. Initially admit
  mutations only when the conversation is authoritatively idle and no route
  change, compaction, or queued admission is pending; explain busy state.
- Bind every async result to runtime generation and page/flow ownership; prompts
  additionally have unique IDs. Session replacement or leaving the page aborts
  pending work and invalidates ownership. Already-committed credentials persist.
- Keep native storage precedence and interprocess locking. Rust never edits
  `auth.json`, persists access/refresh tokens, or adds credentials to shared
  frontend config. Display metadata without resolving/exporting secrets.
- Secrets and manual callback input are ephemeral, masked, not prefilled, and
  dropped on submit/close. Exclude them from Debug output, session/timeline
  messages, execution history, snapshots, and screen-copy source. Do not promise
  memory zeroization from merely dropping a string.
- Authorization URLs/device codes remain in the authentication accessory with
  explicit open/copy actions. Browser launch accepts validated HTTP(S) URLs via
  argument-safe platform operations, never shell interpolation. Clipboard writes
  and browser launch are user-directed, not model-triggered.
- Keep diagnostics bounded and sanitize before they reach Rust logs/UI; do not
  serialize arbitrary native errors or credential-bearing SDK results.
- Negotiate bridge protocol and native capabilities. An incompatible runtime
  leaves existing credential reuse working and offers native Pi login guidance;
  it must not leave the page in a permanent loading state.
- Resolve the helper SDK from the package directory reported by the companion
  extension running inside the selected Pi process. The companion also reports
  Node executable, agent directory, cwd, session ID, Pi version, and current
  project trust.
- Ship both scripts as embedded e-pi assets, materialize them in an owned
  temporary directory for the process lifetime, and pass the companion through
  Pi's explicit extension option. No global installation or source checkout path
  is required.
- Helper communication uses inherited stdio only. There is no socket or shared
  endpoint to authenticate; OS process-handle ownership is the channel boundary.
  Metadata/control deadlines are 15 seconds, shutdown is 3 seconds, and native
  user authorization remains unbounded except for provider expiry and explicit
  cancellation.

## 4. Algorithms and rules

1. Open the page without materializing a draft session. Request safe provider
   metadata from the effective runtime context, not from available models alone.
   Resolve an optional provider argument by stable ID or unambiguous native name.
   Ambiguous/unknown references remain local errors or selections.
2. On explicit method selection, verify catalog generation and idle admission,
   create an abortable flow, and call the native login implementation. Never
   infer a provider-specific sequence in Rust.
3. Forward each native prompt/notification through authentication-owned values.
   The native implementation owns callback and device polling. Frontend redraws
   remain deadline-driven and cannot impose a polling ticker on the idle runner.
4. Accept each reply only for the current flow/prompt. Withdraw a manual prompt
   when its native per-prompt signal aborts; ignore later answers for that prompt.
   Whole-flow cancellation aborts all native waiters and external resources.
5. After native credential commit, synchronize the original Pi model/auth state
   before announcing readiness. Querying its old snapshot is not synchronization.
   Preserve the user's selected model; when no usable model exists, direct the
   user to the refreshed picker rather than inventing a default.
6. Report credential commit and synchronization separately. A catalog-network
   failure can retain cached models and a warning after local synchronization.
   A local-sync failure offers a non-mutating refresh retry, not automatic login.
   After disconnect/timeout with uncertain commit, reconcile status before any
   further mutation; never claim rollback.
7. Closing, replacement, or child EOF tears down pending interaction, drops
   secret buffers, and rejects stale completions. Cleanup has a finite deadline
   and escalates only against processes owned by this frontend. Ordinary session
   content, drafts, and queues are unchanged.

Reuse bounded JSONL decoding. Helper and Pi records are limited before parsing;
helper command/event channels each hold at most 64 records. Startup, context,
catalog and live-runtime refresh each have a 15-second deadline; shutdown has a
3-second deadline. User authorization waiting is separate and respects provider
expiry/cancellation. One active mutation is allowed; a second start fails locally.
Helper disconnect makes an unacknowledged mutation `unknown`, invalidates the
catalog generation, and requires status reconciliation before another mutation.
The inherited stdio channel has no externally addressable endpoint. No unbounded
queues, raw stdout debug prints, or blanket credential retries are permitted.

Existing prompt queue, session persistence, OAuth, and model-selection algorithms
remain authoritative. This change adapts authentication interaction; it does not
replace those algorithms.

## 5. Fixed decisions and discretion

Fixed product/boundary choices: in-session commands only; native credential and
authorization ownership; dynamic providers/methods; protected non-conversation
interaction; DSH compatibility; no automatic runtime fork or replacement.

The design owner approved excluding later runtime provider registrations after
reviewing [the validation evidence](validation-report.md). That scope adjustment
resolves the former integration blocker. Public live refresh remains mandatory.

Local discretion: Rust/TypeScript private names, helper file
layout within adapter ownership, and reuse of neutral page components, provided
focus, masking, copy, and lifecycle behavior remain intact. Do not add a universal
form framework or a second conversation runtime solely to simplify this feature.

## 6. Verification and documentation impact

Use isolated temporary agent directories and fake providers/credential stores.
Do not read or mutate developer credentials or initiate real account login during
automated tests. Native account authorization is an explicit user-assisted smoke
test, with untested providers reported honestly.

Required risk-based checks:

- Catalog/method discovery without credentials, model-less and extension
  providers, rejected-project trust, custom runtime/agent paths, and package
  identity mismatch.
- Native prompt mapping including empty values, option IDs, multi-field setup,
  device waiting, callback-versus-manual races, and prompt withdrawal.
- Cancellation, stale replies, busy admission, page/session replacement, EOF,
  bounded malformed frames, write failure, uncertain commit, and failed refresh.
- Commit and live-model availability, logout exposing unchanged environment
  fallback, concurrent native credential writes/refresh, and no session/queue
  mutation or new blank history entries.
- Secret absence from rendered/copy output and logs/history, DSH API-key/proxy
  regressions, and unaffected generic extension questions.
- Packaged helper startup from an extracted e-pi crate on Windows; non-Windows
  launch/path behavior where supported. Inspect assets, not only workspace runs.

Follow [readme/testing.md](../../../../../readme/testing.md): scoped Rust modules/integration tests and JS
`node:test` for any new helper. Use architecture tests for forbidden dependencies.
For a large cross-cutting implementation run workspace formatting and Clippy;
package creation alone requires neither compilation nor broad Rust tests.

On delivery update current architecture with the approved bridge owner and
lifecycle; runtime/adapter contracts with authentication transport and locking;
interaction/session contracts with command/page lifecycle and cancellation;
configuration/storage and presentation contracts with credential ownership and
sensitive-data/copy rules. Update [README](../../../../../README.md), [e-pi README](../../../../../crates/e-pi/README.md), runtime guidance,
and effective help/locales for the actual shipped workflow. Add an ADR only if
the approved integration needs durable rationale not captured in contracts.

Do not change current architecture/specs during proposal creation: none of the
new behavior is delivered. No DSH wire change is planned; if one becomes
necessary, revisit scope and run its generated-contract checks explicitly.
