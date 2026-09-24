# Implementation design

The Windows-first recording workflow has been exercised with a release `pie`. The confirmed scope and deferred duration target are maintained in [the proposal](../proposal.md). Do not complete or archive this change while the final 90-second treatment is deferred.

## 1. Baseline and goals

The repository currently separates provider-neutral rendering/input in `e-tui` from Pi process/protocol effects in `e-pi`. Relevant current contracts are [runtime and adapters](../../../../specs/runtime-and-adapters.md), [presentation](../../../../specs/presentation.md), [interaction and sessions](../../../../specs/interaction-and-sessions.md), and [configuration and storage](../../../../specs/configuration-and-storage.md).

Existing entry points:

- [crates/e-pi/src/main.rs](../../../../../crates/e-pi/src/main.rs) accepts `--pi <executable>`; [`process.rs`](../../../../../crates/e-pi/src/process.rs) owns JSONL child-process transport. These are existing capabilities, not an approved replay or observation API for this workflow.
- [crates/e-tui/src/runtime/command.rs](../../../../../crates/e-tui/src/runtime/command.rs) implements `set-default-effort`. Supported levels come from the selected model's catalog entry.
- [readme/key-mapping.md](../../../../../readme/key-mapping.md) describes model letter marks and Reading navigation. Default Reading entry is Ctrl+R on Windows; Shift+letter assigns a model mark. Effective mappings must be checked rather than assumed.
- [crates/e-pi/src/config.rs](../../../../../crates/e-pi/src/config.rs) owns shared frontend configuration and resume-state paths. Changing cwd alone does not isolate these stores.
- [crates/e-dsh/examples/input_probe.rs](../../../../../crates/e-dsh/examples/input_probe.rs) exercises the shared Windows raw-input path.

Earlier feasibility checks used the existing input-probe binary, `tui-test 0.1.0-beta.2`, and FFmpeg 8.1. They produced a 5.27-second H.264 MP4 at 30 FPS and confirmed English/Chinese input and readable Chinese glyphs in an extracted frame. Ordinary Ctrl+Backspace injection lost its modifier; explicit VT sequences correctly conveyed Ctrl+Backspace and Shift+Enter to that probe.

Those checks did not validate a complete `pie` recording, the preferred model's availability, Reading/Preview visuals, mouse input, fork/resume, or a 90-second feature cut. `tui-test` re-renders terminal output; it does not reproduce Windows Terminal window pixels or decorations exactly. No relevant source modifications were present when this package was created.

Baseline mismatch observed during execution: a freshly built debug `pie` launched through `tui-test` with temporary APPDATA, LOCALAPPDATA, and PI_CODING_AGENT_SESSION_DIR overrides showed its initial screen, then exited with code 101. The recorded PTY output ends with `unhandled typed TimelineRecord family: UsageCost { usd_nanos: 0 }` at [the session reducer](../../../../../crates/e-tui/src/runtime/state/session.rs) (line 195). No model prompt was sent. This contradicts the assumption that the proposed debug executable can be driven for even a minimal recording. The cause of this projection assertion has not been diagnosed. A freshly built release `pie` remained running with the same environment overrides, but this does not prove that the eventual recording workflow succeeds. The debug assertion must not be silently removed.

A subsequent release-binary probe encountered a different operator error: Git Bash/MSYS rewrote the model slash-command CLI argument into an absolute path under the Git installation. `tui-test submit` sent the rewritten string as a real model prompt. One unintended generation occurred; the isolated session was closed and its temporary data removed. Future probes must avoid Git Bash argument conversion (for example, use a key binding to open the model menu), inspect typed text before submission, and cap actual model calls. Do not append `--json` after `tui-test key press` keys: the trailing option was observed as literal key input by the Windows input probe; put global options before the command. This probe is not acceptance evidence for the recording scenario.

A read-only follow-up opened the model menu with Ctrl+L and the Settings page using Node-spawned literal input. Settings displayed the real user frontend config path under AppData/Roaming/e despite a temporary APPDATA override. The preferred route was visible as `Deepseek Flash max` in the catalog, but its exact route ID and supported effort were not confirmed. The native session directory override isolates sessions, but the current frontend config path still resolves to the real user config; changing marks or default efforts would therefore modify shared settings. No such config changes were made in this probe. The user subsequently approved recording with the existing frontend configuration and a freshly built release executable. Before modifying settings, the driver must snapshot affected config, avoid unrelated changes, and have a safe restoration/concurrency policy. Do not rely on APPDATA alone.

The native catalog query `pi --list-models deepseek` returned only `codemaker/deepseek-flash`, not the requested `deepseek/deepseek-flash`. This is a route-identity mismatch, not permission to silently swap providers. The user approved the exact available route `codemaker/deepseek-flash` for this run and requested a required `--model` option for exact provider/model identities on other machines. The driver must reject missing routes before any paid call. This first pass uses up to four calls per run, a 180-second per-call timeout, and no automatic retry.

## 2. Overall approach

The driver runs outside the frontend: it starts a real release `pie` in `tui-test`, records five chapters (working/tool Preview, Reading, saved effort, model mark and temporary route, fork/resume), and uses FFmpeg to concatenate them without cutting or speeding up content. It types commands and prompts character by character with at least 100 ms between characters, holds the completed input for one second before Enter, and spaces menu/navigation keys by one second. No renderer, adapter, or provider protocol is changed. Live generation is used; replay is not part of this version.

## 3. APIs and data model

The [recording driver](../../../../../tools/record-demo.mjs) takes `--pie`, required `--model` identity, and `--output`; optional `--compare-model` selects a distinct verified route, and `--preflight` checks the tools and live Pi catalog without generation. The synthetic Rust workspace lives under Windows Public so the recorded footer does not reveal a personal home directory. Native sessions and temporary config backups live under the current user's temp directory. The driver uses the existing frontend config with ownership-aware backup and restoration; Pi retains its native credentials. It emits an MP4 and JSON sidecar.

The driver verifies typed composer text before Enter, reads native session JSONL for settled assistant messages and fork parent metadata, and checks the visible route and resume ancestry. It aborts rather than treating an incomplete wait as success. Native lifecycle observation through a wrapper was not needed. A missing main or comparison route is never substituted silently.

## 4. Algorithms and rules

The driver preflights routes and a supported effort, runs at most four model calls per attempt with a three-minute deadline per call and no automatic retry, and checks application state before each chapter transition. The scenario currently makes two model requests. A no-argument fork restores the selected prompt to the composer; the driver waits for that draft, clears it, then opens resume. Each typed character has at least a 100 ms pause before the next, and the completed line has a one-second hold before Enter. Menu/navigation keys are spaced by one second, while model output remains at its native speed.

The exporter validates playable chapter video and reports the final duration, then concatenates chapters at normal speed. It does not guess idle intervals from spinner redraws. A preview longer than 90 seconds is allowed for pacing review and flagged in the JSON sidecar; verified wait-only trimming and renewed duration enforcement remain deferred.

## 5. Fixed decisions and discretion

The exercised setup uses the release binary, 120×36 cells, 30 FPS, a short read-only Rust example, medium effort where supported, a verified resting comparison route, and no subtitles or audio. Existing user theme and credentials are used. The debug-only projection assertion remains unresolved and is not part of this recording workflow. No application behavior change was made to simplify filming. The duration target is deferred by the user, not silently waived.

## 6. Verification and documentation impact

The earlier one-second instruction-gap preview produced a 66.37-second 2556×1712 H.264 MP4 at 30 FPS and was sampled across all five scenes. After switching prompts and commands to 100 ms character-by-character input, the driver produced a 103.2-second preview with no timing cuts; per the user's request, that artifact is reserved for user visual acceptance rather than assistant inspection. Native route restoration and fork ancestry are still enforced by application-state checks during the run. The JSON sidecar reports both pacing values, duration, and no edits. `node --check`, catalog-only preflight, `ffprobe` when inspection is requested, `git diff --check`, and `doco check` are the scoped checks; no tests were added or modified.

No current architecture or specification boundary changed. The supported workflow is documented in [the demo recording guide](../../../../../readme/demo-recording.md). A reproducible under-90-second export for slow model responses remains open and must be addressed before completing this change.
