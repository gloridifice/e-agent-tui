# Implementation design

Discussion draft, not ready for implementation. The confirmed scope and decision register are maintained in [the proposal](../proposal.md); the design below records candidates rather than silently settling those decisions.

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

## 2. Overall approach

Candidate flow for Q1 and Q6:

```text
scenario driver -> tui-test -> real pie -> real Pi/model
                       |
                       +-> raw recording + scene boundaries
                                      |
                                      v
                            FFmpeg edit/export -> MP4
```

Keep orchestration and video processing outside the frontend unless a separately reviewed need for an observation interface is established. No new renderer or provider payload path into `e-tui` is proposed.

Live generation plus state-driven chapter boundaries is the leading candidate. Backend replay remains an unselected alternative; it is not part of an approved first version.

Suggested storyboard for discussion:

| Finished time | Scene | Candidate visible evidence |
| --- | --- | --- |
| 0–22 s | Working interface | Input, Thinking animation, tool activity, streamed answer, Preview. |
| 22–36 s | Reading | Enter Reading, navigate blocks/items, inspect code, exit. |
| 36–49 s | Effort configuration | Save a supported default, show its menu annotation, reselect and observe application. Subject to Q2. |
| 49–69 s | Marks and temporary route | Assign a mark, select a comparison route, submit a marked Flash prompt, observe restoration. |
| 69–85 s | Fork and resume | Select a historical fork point, create the branch, show ancestry in /resume, return to the parent. |
| 85–88 s | Closing hold | Leave a readable settled interface. |

The chapter order, allocations, two-generation limit, and closing hold are suggestions, not approved constraints. Prompts requesting short answers reduce content risk but cannot guarantee latency or output length.

## 3. APIs and data model

No new CLI, scenario format, file layout, or observation API is approved. A later design should specify:

- How launch settings, ordinary typing, explicit key sequences, waits, reading holds, and chapter boundaries are represented.
- Which signal proves prompt admission, native run settlement, temporary-route restoration, and completed session replacement. Visible answer text alone is not sufficient for all four.
- Whether native lifecycle events can be observed through a transparent wrapper without changing protocol behavior, or whether screen-based conditions suffice. Any side-channel metadata must stay out of RPC stdout.
- What raw recording and scene metadata are retained, where outputs live, and how partial or failed runs are identified.
- How the driver owns its child processes and cleans up only its own sessions; how frontend configuration, model marks/defaults, native sessions, and credentials are isolated or safely restored.

The model route and effort must be checked against the live catalog before paid generation. A comparison route must not be invented or silently substituted.

## 4. Algorithms and rules

Candidate execution sequence, subject to the open decisions:

1. Verify binaries, model routes, effort support, key delivery, layout, and isolation without starting a model task.
2. Start a fresh recording environment and drive the approved scenario.
3. Advance only when the relevant application state is confirmed, then add a deliberate viewing hold. Record chapter boundaries independently from model latency.
4. Use finite wait and retry budgets. Decide whether failure aborts the whole run or permits chapter re-recording; chapter retries must reconstruct the required session/model state.
5. Fit chapters to the approved budget by editing only designated intervals. Preserve visible work at normal speed and disclose compressed waits if that strategy is selected.
6. Export the video, check duration/encoding, inspect each feature scene, and clean up owned processes and temporary state.

Spinner redraws are not terminal inactivity: automatic idle-gap compression alone cannot bound a model wait. Model settlement also does not necessarily mean presentation reveal or temporary-route restoration has completed. A failed wait must not be treated as successful scene completion.

For a no-argument /fork, the selected prompt returns to the composer. The driver must deliberately handle that draft before entering /resume, without accidentally sending it or creating another model turn.

## 5. Fixed decisions and discretion

Only the confirmed scope in the proposal is fixed. Q1–Q6 remain blocking discussion items. The proposed timing strategy, prompts, comparison model, recording parameters, and APIs are not implementation discretion yet.

Before handoff, resolve those items, validate the selected routes/efforts, and replace this candidate design with an executable one. No application behavior change is authorized merely to simplify filming.

## 6. Verification and documentation impact

Package creation requires Doco structural validation and link review only. It does not call for Rust/bridge tests or further model calls.

After implementation is separately authorized, verify real `pie` input/rendering, all five scene outcomes, actual temporary-route restoration, native fork ancestry, encoding, duration at or below 90 seconds, and privacy/cleanup. Use existing probes and manual artifact inspection; no tests are to be added or modified without explicit user authorization.

Current architecture/spec changes: none for this draft. If a recording command and supported workflow are delivered later, document them under [readme/](../../../../../readme/README.md) and link them from the appropriate operational index. Reassess current contracts only if an approved implementation introduces a cross-boundary interface or changes a documented invariant.
