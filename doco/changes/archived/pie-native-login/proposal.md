<!-- doco:managed template=v1 -->
# pie-native-login

## Purpose

Allow users to authenticate with Pi's native provider login methods from inside
`pie`, without leaving the frontend to configure credentials in a separate Pi
session. Today `pie` reuses existing Pi credentials but rejects all login
requests, and its shared login page only represents API-key and DSH proxy setup.

Native authentication is more than an API-key field. Providers can request
browser authorization, device codes, manual callback input, method selection,
and multiple configuration fields. Pi extensions can register additional
providers and authentication flows. Reusing Pi's implementation avoids a second
OAuth implementation and a separate credential store.

Research against installed Pi 0.85.1 found native SDK authentication APIs but no
login/logout RPC commands. The SDK-to-frontend bridge therefore needs a verified
integration design before feature implementation. This is a validation-gated
feature proposal, not a claim that an authentication helper already provides
native compatibility.

## Scope and acceptance

### Deliverables

- Support in-session **/login** and **/login \<provider\>** with dynamically discovered
  authentication methods, plus **/logout** for stored credentials.
- Present Pi-owned text, secret, selection, and manual-code prompts; authorization
  URLs, device codes, informational guidance, and progress; cancellation and
  terminal results.
- Delegate login, credential persistence, and token refresh to supported Pi APIs.
  Continue using Pi's effective agent directory and native credential resolution.
- Include Pi AI's built-in and configured providers plus user extension providers
  registered during extension factory loading, including providers that have no
  available models before login.
- Synchronize the running Pi child's authentication and model availability after
  credential changes without losing the current session, input draft, or queues.
- Keep shared frontend authentication state provider-neutral and preserve DSH's
  existing API-key and proxy workflows.

### Validation gate

First validate a Pi SDK authentication helper while retaining the official
`pi --mode rpc` conversation runtime. Prove runtime/package identity, effective
configuration and project trust, extension-provider discovery, protected
interaction transport, and synchronization of the live Pi process.

A bare `ModelRuntime.create()` in another process is not sufficient evidence of
startup extension parity. Requerying `get_available_models` is not sufficient
evidence of refresh. The helper may reconstruct user extensions once through the
same trusted resource paths, but it does not mirror later in-memory registrations.
Do not access Pi private fields, fork its runtime, or replace its launcher with an
SDK host.

The implementation plan must close these blocking choices before production
implementation is handed off. Creation of this package authorizes neither the
validation work nor feature implementation.

### Observable acceptance

1. A supported runtime exposes its eligible login providers and methods without a
   hard-coded Rust provider list or requiring existing credentials/models.
   Eligible providers are Pi AI built-ins, `models.json` providers, and user
   extension providers produced during a separate trusted factory load.
   **/login \<provider\>** selects the matching provider and offers a method choice
   when necessary; an unknown provider never becomes a model prompt.
2. The interaction layer supports browser callback and manual-input alternatives,
   device-code waiting, and multi-step API-key/configuration setup. The initial
   validation matrix includes ordinary API key, Codex browser/device modes, Kimi
   device login, Cloudflare multi-field setup, and an extension provider.
3. Login/logout uses native Pi credential storage and locking. Existing
   credential-source precedence is preserved. Logout removes only stored
   credentials, leaving environment variables and `models.json` unchanged.
4. Successful changes become visible to the existing conversation runtime and
   its model picker without restarting it. Credential commit, local runtime
   synchronization, and remote catalog refresh are reported separately; a refresh
   failure never triggers automatic reauthentication.
5. Closing the page, cancellation, session replacement, or process failure
   terminates or invalidates the associated interaction. A successful browser
   callback dismisses a superseded manual-input prompt. Late events cannot alter
   a newer page or session, and cancellation does not pretend to undo a completed
   credential write.
6. Secret input and returned credentials do not enter model messages, session
   entries, execution history, diagnostic logs, or screen-copy output. Browser
   URLs and device codes have explicit user-directed open/copy actions rather
   than leaking through ordinary transcript notifications.
7. Missing or incompatible bridge support produces an actionable message and
   preserves normal use of already-configured Pi credentials. DSH login and
   generic Pi extension questions continue working unchanged.

### Non-goals

- A shell-level `pie login`/`pie logout` subcommand in this change; the scope is
  the commands inside the `pie` frontend.
- Reimplementing OAuth endpoints, token refresh, credential files, or native
  provider configuration rules in Rust.
- Replacing the official Pi conversation runtime, patching installed Pi files,
  or treating terminal handoff to native Pi as completed in-frontend support.
- Adding DSH-style proxy CRUD to Pi or recreating the **/llama** model manager.
- Multiple stored accounts per provider, credential export, or automatic login
  initiated by the model.
- Guaranteeing provider-side authorization availability or testing every live
  provider account automatically.
- Providers registered only after the conversation session has started, including
  stateful runtime registrations that cannot be reconstructed by loading the
  extension factory once; Pi built-in extensions not exported through the public
  SDK factory surface are subject to the same limitation.

### Intended contract changes

On delivery, current documents will describe the adapter-owned authentication
bridge and lifecycle, provider-neutral login interactions and cancellation,
sensitive-data exclusions, native storage ownership, and post-login runtime
synchronization. User guidance will describe the supported commands and runtime
compatibility requirements. Existing DSH wire and credential formats are not
intended to change.

Implementation planning, dependent work, and verification were completed before
archival. This proposal remains the scope and acceptance authority after working
files are removed.

## Result

Delivered for the approved narrowed scope. Validation used isolated fake credentials and did
not authorize or perform a live provider login. It covered factory-time provider discovery,
protected prompt flows, credential persistence, live runtime synchronization, cancellation,
redaction, packaging, and the documented repository-level check failures. Unsupported later
runtime provider registrations and non-exported Pi built-in extension factories remain
limitations rather than delivered behavior.
