# SDK authentication bridge validation

## Result

The separate SDK-helper candidate is approved for the narrowed scope. It can
reuse native credential operations and trigger a supported refresh in the
original RPC process through a companion extension. It cannot preserve provider
registrations made only in the live extension instance; those registrations are
now an explicit non-goal.

No real provider account was contacted and no developer credential file was
read or changed. The executable probe uses a temporary `PI_CODING_AGENT_DIR`, a
fake provider, and an invalid non-contacted model endpoint.

## Reproduction

Runtime under test:

- `@earendil-works/pi-coding-agent` 0.85.1
- Node.js 25.8.0
- Windows
- package root passed explicitly to both the CLI child and SDK import

Command:

```text
node doco/changes/active/pie-native-login/work/validation/probe.mjs C:/Users/linyifan05/scoop/persist/nodejs/bin/node_modules/@earendil-works/pi-coding-agent
```

Observed output:

```json
{
  "isolatedAgentDir": true,
  "factoryProviderVisibleToHelper": true,
  "lateProviderVisibleToHelper": false,
  "nativePromptKinds": ["select", "secret", "text"],
  "liveRpcRefreshViaPublicExtensionApi": true
}
```

The probe and fixture are retained under [validation/](validation/) so the
result can be repeated against another selected Pi installation.

## Evidence by gate obligation

1. **Runtime/package identity — implemented.** The companion reports its public
   Pi package root and active Node executable from inside the selected child. The
   helper imports the SDK from that root; it does not infer a global installation
   from `PATH` or a wrapper command.
2. **Agent directory/cwd/trust — implemented and exercised.** The companion
   reports the native agent directory, cwd, session ID, and
   `ctx.isProjectTrusted()` decision.
   The helper supplies that decision to the public resource-loader callback. An
   isolated untrusted workspace confirmed that its project-local extension does
   not enter the provider catalog.
3. **Extension-provider boundary — accepted.** A provider registered by a user
   extension factory appears in both processes. A provider registered later by a
   command exists only in the original extension/runtime instance. The narrowed
   scope explicitly excludes the latter and warns that loading a factory in both
   processes may repeat factory side effects.
4. **Protected interaction — implemented and exercised.** The production helper
   uses inherited JSONL stdio, 256 KiB record limits, channels capped at 64, one
   active mutation, correlated flow/prompt IDs, cancellation, and bounded
   diagnostics. Arbitrary provider-thrown errors are projected to generic safe
   outcomes; fake secrets are not retained by frontend state after reply or
   cancellation.
5. **Live process synchronization — passed for startup providers.** After the
   helper committed a fake key, `get_available_models` in the original RPC
   process remained stale. A companion extension command then called public,
   provider-scoped `ctx.modelRegistry.refresh({ allowNetwork: false })`; the existing
   RPC process exposed the provider model without restart or session replacement.
   A separate network-enabled refresh pass reports completion or a non-fatal
   warning. This also confirms that merely requerying `get_available_models` is
   insufficient.
6. **Asset shipping/negotiation — implemented.** Rust embeds the helper and
   companion with `include_bytes!`, materializes them in an owned temporary
   directory, and injects only the companion into the selected RPC process.
   `cargo package --list` includes both JavaScript source assets.

Source inspection also confirms the intended native interaction vocabulary:
Pi AI `AuthInteraction` supports `text`, `secret`, `select`, `manual_code`,
`auth_url`, `device_code`, `info`, `progress`, whole-flow abort, and per-prompt
abort. Built-in Codex supplies browser/device selection, Kimi supplies device
code, and Cloudflare/Vertex supply multi-field setup. These version-specific
facts were inspected but live network authorization was intentionally not run.

## Core mismatch

### Original assumption

A second SDK process could load the same effective provider set as the existing
Pi RPC child, perform native login/logout, and ask that child to refresh.

### Observed facts

- The original child owns an in-memory `ModelRuntime` composed with provider
  registrations from its current extension instances.
- A second process can independently rebuild factory-time registrations only.
  Runtime registrations and extension state are neither persistent nor
  serializable in the general case.
- RPC 0.85.1 has no authentication/catalog command that exposes this runtime.
- Public `ExtensionContext.modelRegistry` can read providers and refresh models,
  but its public `ModelRegistry` has no login/logout or credential-mutation API.
- Reaching its private runtime or importing internal `AuthStorage` would violate
  the approved supported-API boundary and still risks credential-bearing IPC.

### Affected scope

The original scope could not pass the gate. The design owner subsequently chose
to support startup/factory providers and explicitly exclude late runtime
registrations, so tasks 1.1 and 1.2 can close without private API use.

### Recommendation and pending decision

Keep the helper/companion topology for the narrowed provider set. A future
supported Pi RPC authentication API can remove duplicate factory loading and the
late-registration limitation without moving credential ownership into Rust.

## Production implementation verification

The extracted production assets were exercised against the installed Pi 0.85.1
package with:

```text
node doco/changes/active/pie-native-login/work/validation/production-probe.mjs C:/Users/linyifan05/scoop/persist/nodejs/bin/node_modules/@earendil-works/pi-coding-agent
```

Observed output:

```json
{
  "companionContext": true,
  "factoryProviderCatalog": true,
  "modelLessProviderCatalog": true,
  "nativePromptKinds": ["select", "secret", "text", "manual_code"],
  "liveRuntimeRefresh": true,
  "remoteCatalogReporting": true,
  "storedCredentialRemoval": true,
  "cancellation": true,
  "diagnosticRedaction": true,
  "environmentFallbackAfterLogout": true,
  "untrustedProjectIsolation": true,
  "committedSynchronizationFailure": true,
  "liveSessionPreserved": true,
  "controlTranscriptExcluded": true
}
```

A real `pie` binary was also driven on Windows with `tui-test` and an empty
isolated `PI_CODING_AGENT_DIR`. It displayed built-in providers and methods,
masked a fake API key, kept that value out of copied-screen matching, settled
cancellation, and exposed an empty removable-credential roster through
`/logout`. Focused frontend tests cover explicit page-local URL open/copy and
device-code copy while preserving the draft session and pending prompt queue.
The production probe also confirms that companion controls preserve the live
session identity and append no conversation messages.

The companion reports local live-runtime synchronization separately from remote
catalog refresh; the isolated run records the expected offline remote-refresh
warning. The fixture also forced native credential commit followed by model-runtime
synchronization failure. The helper classified the result as committed but not
synchronized, omitted the fake secret from diagnostics, and removed the stored
fixture credential during cleanup. Host-side tests verify that a live-runtime
refresh failure reports the committed state and directs the user to a frontend
restart for a non-mutating synchronization retry, because `/reload` only
reloads frontend configuration and cannot re-drive the companion refresh.

No real provider login was attempted. Live Codex, Kimi, and Cloudflare accounts
and non-Windows launcher behavior were not tested; those are recorded platform/
provider coverage limits rather than blockers for the approved narrowed scope.
