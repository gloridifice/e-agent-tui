# Execution tasks

A Windows-first five-scene recording workflow and paced preview have been exercised. The user deferred enforcing the eventual 90-second limit; that task remains open. Do not complete or archive the change.

- [x] 1.1 Resolve the first recording design with the user
  - Design: [recording design](implement.md), [decision register Q1–Q6](../proposal.md#open-decisions).
  - Acceptance: Confirm release `pie`, exact main route via CLI, catalog-verified comparison/effort, real generation, shared config with safe restoration, five scenes, one-second instruction pacing, and deferred duration enforcement.
  - Verification: The user approved release execution, existing config, multiple model calls, `codemaker/deepseek-flash` for this run, no subtitles, and the one-second instruction gap; catalog preflight verified the selected routes and effort.

- [x] 2.1 Build and inspect a minimal real-application recording
  - Dependencies: 1.1
  - Design: [baseline and approach](implement.md#1-baseline-and-goals).
  - Acceptance: Drive release `pie` through the synthetic read-only Rust task, verify model settlement and input delivery, and inspect working/Reading visuals without substituting the approved route.
  - Verification: Live catalog preflight and the recorded working/Reading chapters showed the approved route, tool use, streamed answer, Preview, and Reading navigation; no test was added.

- [x] 2.2 Implement the approved five-feature recording and export workflow
  - Dependencies: 2.1
  - Design: [interfaces](implement.md#3-apis-and-data-model), [execution rules](implement.md#4-algorithms-and-rules).
  - Acceptance: The entry point generates five live chapters with one-second visible instruction gaps, bounded model requests, safe settings restoration, verified temporary-route return and native fork/resume ancestry, and MP4/JSON output without editing waits.
  - Verification: `node tools/record-demo.mjs --pie target/release/pie.exe --model codemaker/deepseek-flash --output <temp MP4>` generated a paced five-scene artifact; the script checked native final messages, restored route, child header, and `(fork)` in resume. No additional model call was made for the fork.

- [x] 3.1 Verify the paced preview and review current-document impact
  - Dependencies: 2.2
  - Design: [verification and documentation](implement.md#6-verification-and-documentation-impact).
  - Acceptance: Check encoding and actual duration, sample all five scenes for readability, verify timing disclosure and synthetic-workspace privacy, and document the public workflow. Do not treat this preview as resolution of the deferred duration target.
  - Verification: `ffprobe` reported H.264/yuv420p, 2556×1712, 30 FPS, 66.37 seconds. Sampled scene frames include tool activity, Reading, `medium` menu annotation, model mark/route restoration, and the resume family tree. The sidecar reports a one-second gap and no cuts; the operational guide is linked from [the readme index](../../../../../readme/README.md).

- [ ] 4.1 Revisit the 90-second target after pacing review
  - Blocked: the user explicitly deferred strict 90-second enforcement for this pacing pass.
  - Acceptance: Agree on verified wait-only cutting or another policy that preserves required interactions, then demonstrate a repeatable compliant export under slow generation. Do not complete or archive until this target is resolved or separately revised by the user.

## Verification

- Build: `cargo build --release -p e-pi --bin pie` succeeded. Debug `pie` had a separate `UsageCost { usd_nanos: 0 }` projection assertion; this workflow uses release instead. An earlier Git Bash probe unintentionally submitted a converted slash command as one model prompt; the resulting temporary data was cleaned and the Node driver avoids shell argument conversion.
- Driver: `node --check tools/record-demo.mjs` and `--preflight` succeeded. The paced live run generated an MP4 plus sidecar; no test files were added or changed.
- Artifact: the earlier instruction-gap preview was checked with `ffprobe` as H.264/yuv420p, 2556×1712, 30 FPS, 66.37 seconds, and its sampled contact sheet showed all five scenes. The newer character-by-character run generated `e-automated-demo-typed-2026.mp4` at 103.2 seconds with a 100 ms character delay and no timing cuts; visual acceptance is left to the user as requested. Sidecars disclose pacing and edit status. Native sessions and settings snapshots were cleaned or restored by the driver; no unrelated repo files were modified.
- Run `doco check automated-demo-recording` and `git diff --check` after these document updates. The deferred 90-second policy remains an open task; mechanical success is not completion or archive authorization.
