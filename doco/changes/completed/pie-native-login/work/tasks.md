<!-- doco:managed template=v1 -->
# Execution tasks

The approved narrowed implementation and planned verification are complete.
Repository-level baseline failures and untested live-account/platform cases are
recorded below. The change remains active pending explicit Doco completion or
archival. See [the proposal](../proposal.md), [implementation design](implement.md),
and [validation report](validation-report.md).

- [x] 1.1 Validate the SDK authentication bridge candidate in isolation
  - Design: [approved integration](implement.md#2-overall-approach)
  - Acceptance: Record reproducible evidence using the selected native runtime and temporary credentials/config. Cover ordinary and multi-field API-key interaction, source-confirm the native browser/device/manual interaction vocabulary, prove factory-time extension discovery and its later-registration boundary, and identify a public path that refreshes the original RPC child's auth/model state without restarting it. Distinguish fixture behavior from live authorization; no real account mutation without explicit user consent.
  - Blocked: none.
  - Scope: The design owner explicitly excluded later runtime registrations after reviewing [validation](validation-report.md).
  - Verification: The command documented in the validation report ran [probe.mjs](validation/probe.mjs) successfully against the installed package. It proved select/secret/text interaction, isolated native persistence, factory-provider discovery, the explicit late-provider limitation, and live RPC refresh through a public extension API. Native Codex/Kimi/Cloudflare interaction forms were source-confirmed; no live provider authorization was run.

- [x] 1.2 Close and approve the production integration design
  - Dependencies: 1.1
  - Acceptance: Record the chosen supported APIs, topology, runtime/package resolution, asset deployment, trust/provider-context handling, protocol negotiation, channel protection, capacities/deadlines, cancellation and commit/synchronization semantics. Resolve former blockers without private APIs.
  - Blocked: none.
  - Verification: [Implementation design](implement.md) now fixes the helper/companion topology, public API boundary, provider eligibility limitation, stdio protection, 256 KiB framing, capacity 64, 15-second control deadlines, 3-second shutdown, single-flow admission, and uncertain-commit recovery.

- [x] 2.1 Define neutral authentication capabilities, requests, events, and page state
  - Dependencies: 1.2
  - Acceptance: Represent dynamic methods, safe status/removability, typed prompts/notifications, withdrawal, correlated replies and outcomes without Pi DTOs or secrets in diagnostic projections. Preserve DSH API-key/proxy behavior through explicit capabilities. Tests protect state ownership, stable option IDs, empty input, cancellation, and stale-event rejection.

- [x] 2.2 Implement and package the native authentication bridge
  - Dependencies: 1.2
  - Acceptance: The approved integration resolves the selected Pi SDK, loads only eligible resources, uses native login/logout and storage, negotiates support, and implements bounded protected transport and abortable lifecycle. Helper/companion assets are included in the published e-pi package. Temporary-home JS tests cover credential commit, failure, cancellation, and diagnostic sanitization; there is no Rust credential-file writer or private Pi API dependency.

- [x] 3.1 Connect e-pi authentication effects and live-runtime synchronization
  - Dependencies: 2.1, 2.2
  - Acceptance: Login requests no longer use the unsupported route for a compatible runtime. Effects run outside UI locks and separately from conversation admission/history. Busy admission, page/session invalidation, EOF, uncertain commit, and cleanup follow the approved design. Login/logout updates the original Pi runtime and model catalog without restart, lost drafts, changed queues, or automatic reauthentication after refresh failure.

- [x] 3.2 Implement the login/logout interaction and command routing
  - Dependencies: 2.1, 3.1
  - Acceptance: **/login**, **/login \<provider\>**, and **/logout** operate locally with provider/method selection, secret-aware input, multi-step prompts, authorization links/device codes, explicit open/copy, waiting/cancel, and precise outcomes. Callback completion dismisses manual input. Unsupported controls are not offered. Existing semantic key mappings and focus behavior remain effective; secrets do not appear in screen-copy or transcript output. DSH login and generic Pi extension questions regressions pass.

- [x] 4.1 Verify end-to-end compatibility and shipped assets
  - Dependencies: 3.1, 3.2
  - Acceptance: Isolated process tests demonstrate native credential reuse, environment fallback after logout, model-less/extension providers, trust isolation, failed synchronization and non-mutating recovery, cancellation races, safe sensitive-data handling, and preserved session/draft/queues. Exercise the extracted crate assets and Windows launcher resolution. Record any explicitly authorized live-provider smoke results separately and list providers/platforms not tested.
  - Verification: The production probe covers stored/native reuse, environment fallback after logout, a model-less factory provider, untrusted-project exclusion, committed synchronization failure and cleanup, cancellation ordering, redaction, and live session identity. A frontend controller test preserves a draft session and queued prompt across auth events. The packaged assets and Windows binary smoke were exercised. No live provider authorization was performed; non-Windows launchers and live Codex/Kimi/Cloudflare accounts were not tested.

- [x] 4.2 Synchronize delivered contracts and user guidance
  - Dependencies: 4.1
  - Acceptance: Current architecture/specs describe only delivered ownership, authentication lifecycle, credential safety, and synchronization contracts. README, e-pi README, runtime guidance, help, and locales match the actual supported workflow and runtime compatibility. Document explicit limitations; do not copy provider registries or implementation history into current specs. No unrelated DSH wire or credential format changes are introduced.

- [x] 5.1 Run final verification and review acceptance
  - Dependencies: 4.1, 4.2
  - Acceptance: Run `doco check pie-native-login`, scoped Rust login/page/adapter tests, e-pi architecture tests, helper tests, and packaged-asset checks selected under readme/testing.md. For the expected cross-cutting implementation, also run workspace formatting and Clippy. Review every proposal acceptance criterion against evidence and report all deferrals. Implementation verification does not authorize completing or archiving this change.

## Verification

Implemented and passing checks:

- `cargo fmt --all -- --check`
- `cargo check -p e-tui -p e-pi -p e-dsh`
- scoped native-auth state, command-routing, screen-copy, adapter, helper-record,
  and e-pi architecture tests
- `cargo clippy -p e-pi --lib --no-deps -- -D warnings`
- `node --check` for the helper, companion, and production probe
- the production probe against Pi 0.85.1, including factory-provider discovery
  with a model-less provider, select/secret/text/manual-code prompts, live RPC
  refresh with stable session identity, separate remote-catalog reporting, stored
  credential removal, environment fallback after
  logout, untrusted-project exclusion, committed-but-unsynchronized recovery,
  cancellation ordering, diagnostic redaction, and transcript-free companion controls
- `cargo package -p e-pi --allow-dirty --no-verify --list`, which includes both
  embedded JavaScript assets
- a Windows `tui-test` smoke using an isolated empty agent directory: provider
  and method selection rendered, secret text stayed masked and absent from screen
  matching, cancellation settled, and **/logout** showed no removable credentials;
  focused tests also cover explicit page-local URL open/copy and device-code copy
- `git diff --check`

Broader checks found repository-level failures outside this change:

- `cargo test --workspace`: the suite reached two unrelated e-tui rendering/
  link-copy failures that also fail when rerun individually; scoped authentication,
  adapter, DSH login, and architecture tests pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: blocked
  by the repository's existing Clippy warning backlog. Clippy without `-D warnings`
  completes.
- The bridge test suite: 107 tests passed; the generated-contract check reports
  pre-existing stale [wire protocol output](../../../../specs/wire-protocol.md).
  This change does not modify the DSH wire contract.

No real provider account was contacted and no developer credential store was read
or changed. The change remains active: implementation and verification do not
authorize Doco completion or archival.
