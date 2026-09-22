# Automated demo recording

## Purpose

Create an automated workflow that drives a relatively fixed input sequence through the real application and produces a feature demonstration video. Re-recording should not require manually repeating every interaction.

LLM output duration is the main unresolved constraint: predictable inputs do not guarantee predictable latency, response length, or tool activity. The finished video must fit within 90 seconds without losing the interactions it is intended to demonstrate.

Status: discussion draft. Only recording the agreed scope and open questions is authorized; implementation, completion, and archive are not authorized.

## Scope and acceptance

### Confirmed scope

- Use `tui-test` and FFmpeg for the recording workflow.
- Automate a relatively fixed interaction sequence that resembles real use and can generate a video artifact directly.
- Keep the finished video at or below 1 minute 30 seconds.
- Cover all five requested feature groups:
  1. The working interface's visual behavior.
  2. Reading mode.
  3. Model effort configuration, requested as **/model MODEL_ID set-effort EFFORT**; see Q2 for the current command distinction.
  4. Letter marks in /model and temporary model selection with **//MODEL_MARK**.
  5. /fork and the /resume page.

### Stated preference

Use **deepseek/deepseek-flash** for model generation. This is the user's preferred route, not a verified catalog entry or an assumption about supported effort levels. No alternative model or effort level has been selected.

### Open decisions

| ID | Question | Current position |
| --- | --- | --- |
| Q1 | How should unpredictable generation time fit the video budget? | Live generation followed by editing waits is the leading suggestion, not an approved design. Event replay was researched but has not been selected. Whether raw recording may exceed 90 seconds, how edits are disclosed, and retry/time/cost limits remain open. |
| Q2 | Which effort behavior should the video demonstrate? | The current command is **/model MODEL_ID set-default-effort EFFORT**, which saves a default without changing the current route. **/effort EFFORT** changes the current session. Confirm the intended behavior and a supported level; no `set-effort` alias is currently planned. |
| Q3 | How should temporary model switching be made visible? | Select a distinct comparison route and mark letter. One suggestion is to keep the comparison route as the resting route and use a marked Flash prompt, so generation still uses Flash while restoration remains visible. This is not yet selected. |
| Q4 | What task, prompts, and session history should be used? | A small read-only Rust example, one substantive answer, one short marked follow-up, and a fork without another generation are suggestions. Exact content, tool activity, fork point, and session names remain open. |
| Q5 | What should the finished video look like? | Resolve UI/content language, terminal dimensions, font, theme, Preview layout, resolution, frame rate, typing pace, captions/audio, and the final chapter budget. An 85–88 second cut is a suggestion, not a second duration requirement. |
| Q6 | What automation interface and execution environment should be supported? | A Windows-first `pie` workflow is the candidate. Decide the entry command, state/completion signals, configuration/session isolation, artifact locations, cleanup, and whether other platforms or DSH are in scope. |

### Acceptance direction

The eventual workflow should produce a playable video within the duration cap, with enough visible interaction to identify each requested feature. Temporary-model coverage should include restoration of the resting route, and fork/resume coverage should make the parent-child relationship visible. Exact scene checks and recording parameters must be agreed before implementation.

The result must not expose private sessions, credentials, or unrelated workspace content. Edited or replayed timing must not be presented as measured live model performance.

### Non-goals and contract impact

This discussion does not authorize changing application commands, input behavior, model routing, Pi session formats, or frontend/adapter ownership. A general-purpose video editor, performance benchmark, and deterministic LLM output are not objectives.

No current architecture or specification change is delivered by creating this package. Existing [interaction](../../../specs/interaction-and-sessions.md), [presentation](../../../specs/presentation.md), and [runtime](../../../specs/runtime-and-adapters.md) contracts remain authoritative. A later approved recording workflow may need an operational guide; that interface is not defined yet.

## Result

Pending — discussion only; no recording workflow has been implemented or delivered by this change.
