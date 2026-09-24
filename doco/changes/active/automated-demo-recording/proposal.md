# Automated demo recording

## Purpose

Create an automated workflow that drives a relatively fixed input sequence through the real application and produces a feature demonstration video. Re-recording should not require manually repeating every interaction.

LLM output duration is the main unresolved constraint: predictable inputs do not guarantee predictable latency, response length, or tool activity. The 90-second target remains the eventual goal, but the user has temporarily deferred its enforcement while reviewing interaction pacing. No demonstrated interaction should be silently cut.

Status: execution in progress. A five-scene recording workflow and playable preview exist; pacing and the deferred duration target are under review. Completion and archive are not authorized.

## Scope and acceptance

### Confirmed scope

- Use `tui-test` and FFmpeg for the recording workflow.
- Automate a relatively fixed interaction sequence that resembles real use and can generate a video artifact directly.
- Target a finished video at or below 1 minute 30 seconds after the current pacing review; do not reject a longer pacing preview solely for its duration.
- Cover all five requested feature groups:
  1. The working interface's visual behavior.
  2. Reading mode.
  3. Model effort configuration, requested as **/model MODEL_ID set-effort EFFORT**; see Q2 for the current command distinction.
  4. Letter marks in /model and temporary model selection with **//MODEL_MARK**.
  5. /fork and the /resume page.

### Stated preference

The [recording driver](../../../../tools/record-demo.mjs) accepts an explicit `--model` provider/model identity so the main route can vary by machine. This run uses **codemaker/deepseek-flash**, approved after catalog verification; there is no implicit substitution for the originally preferred **deepseek/deepseek-flash**. Validate route and effort against the live catalog before recording.

### Open decisions

| ID | Question | Current position |
| --- | --- | --- |
| Q1 | How should unpredictable generation time fit the video budget? | The first workflow records live output and preserves every frame. For now, a preview over 90 seconds is allowed and reported, not rejected; verified wait-only cutting remains open. Generation has a finite per-run call budget and deadline with no automatic retry. |
| Q2 | Which effort behavior should the video demonstrate? | Demonstrate **/model MODEL_ID set-default-effort EFFORT**, which saves a default without changing the current route; select a supported level from the live catalog before recording. No `set-effort` alias is planned. |
| Q3 | How should temporary model switching be made visible? | Use a distinct catalog-verified resting route (codemaker/qwen3.8-max in this run), assign an unused letter to the main route, submit a marked main-route prompt, and visibly verify restoration to the resting route. |
| Q4 | What task, prompts, and session history should be used? | Use a synthetic one-line read-only Rust example, one tool-backed explanation, a short marked follow-up, then fork from a selected prior user message without another generation. Native sessions are disposable and isolated from the user's saved sessions. |
| Q5 | What should the finished video look like? | No subtitles or audio for the first version. The current recording uses a 120×36 terminal at 30 FPS and the user's theme; type commands and prompts one character at a time (at least 100 ms between characters), hold the completed line for one second before submission, and leave one second between menu/navigation keys. Reassess pacing and chapter budget before enforcing the 90-second target. |
| Q6 | What automation interface and execution environment should be supported? | Use the [recording driver](../../../../tools/record-demo.mjs) from the repository root with `--pie`, `--output`, and required `--model` provider/model identity. Use a freshly built release `pie` and the existing user frontend configuration, restoring owned changes safely. The first workflow targets Windows `pie`; DSH and other platforms are out of scope. |

### Acceptance direction

The eventual workflow should produce a playable video within the duration cap once that target is resumed; pacing previews may exceed it while retaining enough visible interaction to identify each requested feature. Temporary-model coverage should include restoration of the resting route, and fork/resume coverage should make the parent-child relationship visible. Exact scene checks and recording parameters must be agreed before implementation.

The result must not expose private sessions, credentials, or unrelated workspace content. Edited or replayed timing must not be presented as measured live model performance.

### Non-goals and contract impact

This discussion does not authorize changing application commands, input behavior, model routing, Pi session formats, or frontend/adapter ownership. A general-purpose video editor, performance benchmark, and deterministic LLM output are not objectives.

No current architecture or specification change is delivered by creating this package. Existing [interaction](../../../specs/interaction-and-sessions.md), [presentation](../../../specs/presentation.md), and [runtime](../../../specs/runtime-and-adapters.md) contracts remain authoritative. A later approved recording workflow may need an operational guide; that interface is not defined yet.

## Result

In progress — the Windows release-`pie` driver now types commands and prompts character by character with a 100 ms inter-character delay, holds completed lines for one second, and spaces menu/navigation keys by one second. It produced `e-automated-demo-typed-2026.mp4` at 103.2 seconds with no timing cuts; visual acceptance is intentionally left to the user. The 90-second target remains deferred pending a safe policy for slow generations. Do not complete or archive this change yet.
