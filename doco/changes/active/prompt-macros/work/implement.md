# Implementation design

## 1. Baseline and goals

Baseline inspected at `825b5a9`; the working tree was clean before creating this package. No existing active change has the same goal. Requirements and scenarios are in [the target contract](specs/prompt-macros.md). This document describes planned APIs, not already implemented features.

Relevant existing owners:

| Entry point | Current behavior and necessary extension |
| --- | --- |
| [e-tui input](../../../../../crates/e-tui/src/input.rs) (`prompt`, `commit`, `refresh_suggest_at_cursor`) | `prompt()` strips trailing newlines; `commit()` records/clears input and classifies shell, slash, or prompt. Completion accepts only slash-prefixed contexts and its Enter path can fill then immediately commit. Macro matching must precede normalization and have distinct acceptance behavior. |
| [Completion ranking](../../../../../crates/e-tui/src/input/completion.rs), [command catalog](../../../../../crates/e-tui/src/command_catalog.rs) | Pure catalog ranking and one built-in registry. Reuse model/effort/skill candidates; add the macro definition and cursor-local contexts without a second registry of commands. |
| [Runtime commands](../../../../../crates/e-tui/src/runtime/command.rs), [terminal controller](../../../../../crates/e-tui/src/runtime/controller/terminal.rs) | Local command dispatch, explicit skill submission, and application of command outcomes currently run from the terminal path. Deferred macros need the same dispatcher and outcome application, not a parallel implementation. |
| [Interaction](../../../../../crates/e-tui/src/interaction.rs), [input controller](../../../../../crates/e-tui/src/runtime/controller/input.rs), [effect controller](../../../../../crates/e-tui/src/runtime/controller/effect.rs) | `PendingPrompt` stores prompt/delivery/model/new-mode; deferred dispatch emits `Input`, `Steer`, or `NewInput` and strips a model mark whenever a model exists. A macro needs an already-expanded typed body and must not be stripped as though it had a mark. |
| [Model controller](../../../../../crates/e-tui/src/runtime/controller/model.rs), [app state](../../../../../crates/e-tui/src/app.rs) | Existing Selecting/Ready/Active/Restoring/RestoreFailed state and exact original-route restoration. Selection currently compares provider/model, not requested effort, and restoration eligibility does not by itself establish completion of arbitrary commands. Extend these barriers instead of creating another model-switch state machine. |
| [Actions](../../../../../crates/e-tui/src/action.rs), [runtime state](../../../../../crates/e-tui/src/runtime/state/mod.rs) | Provider-neutral requests and prompt/skill interpretation; active command accounting and agent/full-idle distinctions. Keep semantic command execution separate from plain prompt submission. |
| [Config](../../../../../crates/e-tui/src/config.rs), [model defaults](../../../../../crates/e-tui/src/model_defaults.rs), [embedded defaults](../../../../../crates/e-tui/assets/default_config.toml) | Strict known-key overlay, validated array precedent, and e-only effort defaults. An ID-keyed empty default table would discard dynamic keys; use an array. |
| [Input rendering](../../../../../crates/e-tui/src/ui/input.rs), [composer region](../../../../../crates/e-tui/src/ui/region/composer.rs), [input layout](../../../../../crates/e-tui/src/input/layout.rs) | Real input is wrapped before cursor painting; shell mode uses Coral and a bottom-rule shell label. Model hints currently enter display wrapping; the new one-row macro hint must not reuse that wrapping behavior. |
| [Theme schema](../../../../../crates/e-tui/src/theme.rs), [Ferra theme](../../../../../crates/e-tui/assets/themes/ferra.toml) | `theme.coral` exists; `theme.activity.label` is the current Umber-equivalent semantic role and `input.status_hint` owns the bottom label tone. No hard-coded RGB or new font-size mechanism is needed. |
| [Pi requests](../../../../../crates/e-pi/src/adapter/request.rs), [responses](../../../../../crates/e-pi/src/adapter/response.rs), [sessions](../../../../../crates/e-pi/src/adapter/session.rs) | `ModelSet` sequences native model/effort operations then refreshes state. Command prompts and explicit skills have special paths. Generic command success currently requests state, unlike the dedicated normalized reload result; route-bearing command settlement needs explicit correlation. |
| [DSH adapter](../../../../../crates/e-dsh/src/bridge/adapter.rs), [bridge dispatcher](../../../../../bridge/src/dispatcher.js) | Ordinary input, commands, and new input are distinct paths. Existing-session skill injection occurs on the command path, while new-input skill handling is already specialized. Sending every macro through ordinary input would break DSH skill behavior. |
| [Pi config](../../../../../crates/e-pi/src/config.rs), [DSH config](../../../../../crates/e-dsh/src/config.rs) | Shared frontend paths and adapter-owned config persistence. Extend the schema and reuse these effects; do not add frontend filesystem access. |

Existing evidence inspected includes the controller's default-effort capture, temporary selection/restoration/failure cases, skill completion with trailing text, and the input renderer's real cursor/model-hint behavior. These do not yet prove macro behavior. No test changes or test execution occurred during creation.

## 2. Overall approach

Keep a pure macro-definition/parser module in `e-tui`, read the registry through the existing frontend configuration owner, and prepare an owned submission before any external effect:

```text
original composer + registry
  -> exact macro lookup before normalization
  -> definition/route snapshot (or ordinary input)
  -> existing single-input classification, expansion disabled on re-entry
  -> local command / shell / prompt admission
  -> optional shared temporary-route barrier
  -> existing adapter effects and authoritative completion
  -> exact restoration before dependent work
```

Use one lookup result for rendering and invocation, but recompute it from the live raw input/config when submitting; a cached candidate or a clipped preview is never executable authority. Do not implement macros by inserting body text into the composer, fabricating a model mark, or replaying keys.

Extract command-outcome application from the terminal controller into a shared synchronous controller helper. Both direct keys and a ready deferred command call it with owned inputs and the same UI context. Never call the locking local-command dispatcher while still holding the state lock used to pick a queued item. Selection/reduction returns owned work, guards are released, then effects execute through the normal runner.

No backend-specific macro implementation, native extension, second prompt queue, persistent worker, or polling loop is needed. Generic command settlement may require small adapter normalization changes; the external DSH/Pi protocols remain unchanged.

## 3. APIs and data model

### Pure schema and parsing

Add a planned `prompt_macros.rs` module beside the existing e-tui config module, a leaf independent of input/UI/runtime and filesystem effects. Planned shapes (private names may vary):

```rust
struct PromptMacro {
    id: String,
    prompt: String,
    model: Option<String>, // canonical provider/model spelling after resolution
    effort: Option<String>,
}
struct PromptMacros(Vec<PromptMacro>);

fn exact_match<'a>(registry: &'a PromptMacros, raw: &str, has_images: bool)
    -> Option<&'a PromptMacro>;
fn parse_definition(line: &str) -> Result<ParsedDefinition<'_>, MacroParseError>;
fn definition_context(line: &str, cursor_byte: usize) -> DefinitionContext;
```

The complete and partial parses share a tokenizer and body-boundary rules. Return borrowed body text and token byte ranges; convert to character indices explicitly at the input boundary. Errors identify the relevant ID, option, missing value, duplicate, or empty body without modifying config. Route resolution uses existing catalog helpers in the caller, not an input dependency inside this leaf.

`Config.prompt_macros` is a validated array with `prompt_macros = []` in embedded defaults. Serialization rejects structurally invalid/duplicate IDs but does not require a live model catalog during config loading. Use the existing catalog resolver for canonical models rather than splitting on an assumed number of slash characters. Unknown saved routes remain visible and diagnosable.

Pass the immutable registry alongside catalogs to production input/composer contexts; do not duplicate it in `CatalogModel` or add a synchronized registry copy to `InputState`. Configuration reload and in-process definitions refresh completion via the current live-config synchronization path.

### Prepared submissions and runtime ownership

Extend the owned pending-submission representation with explicit origin/kind, rather than inferring behavior from `model.is_some()`. Retain existing queue operations, priority, cancellation, and display owners. Planned semantic information:

- original composer input for recovery/history;
- expanded complete body;
- submission kind: prompt, explicit skill, backend command, or immediate local/shell action;
- optional resolved `ModelSelection` and whether an effort must be confirmed;
- origin distinguishing ordinary input, stripped model mark, and macro expansion;
- existing new-mode/materialization metadata;
- session/draft generation and a non-reused local execution identity for a routed invocation.

The queue owns a Record's prompt and optional PromptState. Explicit routes/defaults are captured at admission; omitted models stay omitted until dispatch. The return baseline is not queued data: capture it only after an adapter state fence has settled previous model/effort mutations. During steering replacement, inherit the original baseline A rather than taking temporary B as a fresh baseline.

An expanded command remains an `AgentRequest::Command` or an existing local action. Explicit skills retain `NewInput` handling for drafts. Prompt and skill Records follow normal requested ASAP/after-turn delivery; a consumed Record may be superseded without whole-run idle. A Record still awaiting native admission/consumption, or an unsettled backend command, keeps the next Record local so model state cannot detach from its queued prompt.

### Confirmation and persistence

Use `UiAction::PersistConfig` outside frontend guards. A successful parsed definition becomes live through the normal config synchronization path. Do not announce a successful disk save before its completion. On a failed write, show that the live definition is unsaved; do not roll back an entire config snapshot over newer edits. Existing configuration errors remain visible.

Normalize command completion through owned agent events. For Pi, associate the native command prompt response and subsequent authoritative state refresh with its session/operation; emit a terminal command outcome for the command phase, while a started model run remains governed by native run settlement. Native prompt admission acknowledgments alone are not whole-run completion. DSH already supplies command results/errors; under the exclusive route window, associate the single outstanding frontend command with the current local owner. Do not expose native IDs as macro schema fields or invent a new wire frame.

## 4. Algorithms and rules

### Definition and exact lookup

1. Parse ID/options without interpreting the body as commands. Stop header parsing at the first body token or explicit separator, then retain the original remainder; remove header separators only.
2. Validate syntax and any explicit target/effort pair. An effort-only definition remains model-independent until invocation. Update the complete same-ID entry, not a partial merge with stale options.
3. For invocation/highlight, first require text-only raw input. Remove only trailing ASCII spaces and perform case-sensitive full lookup. This runs before `InputState::prompt()` can erase a newline.
4. At commit, record the original typed input once, resolve the target, and prepare the owned expanded body. Classify with recursive macro lookup disabled. Failure restores the original draft or retains a visible failed item if newer input exists.

### Routing, commands, and failure

Follow [R3/R4](specs/prompt-macros.md#r3-command-and-skill-execution). Put route compatibility classification next to the single command registry/dispatcher, not in renderer/completion code. Explicit skill calls and backend-forwarded commands are eligible. Local-only actions and known route/session/resource/exit controls reject inapplicable/contradictory override options before effects; no-override aliases use their unchanged handlers. Existing command capabilities and native guards still decide execution.

For a routed invocation:

1. Capture the complete body and optional PromptState at admission, leaving an omitted model unresolved. Preserve requested delivery and existing compaction/clear barriers.
2. At dispatch, create a non-reused Record owner. For the first temporary state, issue an adapter state fence behind earlier model/effort controls before capturing A. Pi uses a correlated state read; DSH serializes a model refresh and pong behind its existing control chain without a new wire frame. A steering successor inherits A and retires its predecessor only after native admission/consumption. Resolve omitted fields against A and reject unavailable targets locally.
3. Reuse `TemporaryModel` phases to select the target. Selection confirmation compares provider/model and the explicit/requested effort when present; no-effort selection retains existing backend default behavior. Confirm restoration against the complete original selection.
4. Dispatch exactly once through the prepared action's owner. A pending command/admission latch prevents the pre-dispatch idle state from immediately triggering restoration. Skills use their existing joint skill/message presentation and native admission lifecycle.
5. Keep the route window until the command phase, if any, has settled and any resulting agent work is authoritatively idle, including backend pending prompts and fallback retry. Do not release on a tool-step end or a prompt-accepted acknowledgment. Commands producing no run complete through their command result plus refreshed state.
6. Admit eligible ASAP successors as Records, including unconfigured ordinary steering and marked prompts. They replace, rather than nest, the current state owner and inherit A; do not restore between B and C. Other conflicting native mutations remain rejected. Local UI actions and cancellation remain available. Existing marked-prompt behavior outside a Record chain stays unchanged.
7. Restore the complete original route and confirm it before releasing subsequent work. A failed selection dispatches no body and still restores after partial mutation. Rejected commands or failed admission retire their own pending latch and preserve recoverable input. Cancellation holds an owned post-interrupt state fence (after Pi abort acknowledgment) before retiring prompt-wait latches; an uncertain refresh remains blocked and may be requested again. It does not replace baseline A or replay the body.
8. Failed restoration leaves the barrier visible and blocked; a later authoritative confirmation of the original route can clear it. Never automatically rerun an uncertain body. On unrelated session replacement discard old ownership; on expected new-draft materialization transfer it through the existing new-input handshake. Late completions must fail the generation/owner check.

Do not special-case the example model ID. Neither the current catalog nor the repository promises its availability.

### Completion

Build a cursor-local completion context with an editable range and a preserved suffix, using the existing replacement/restoration machinery. The partial macro parser identifies ID, option name, model value, effort value, or body-start command context. Read the whole header to resolve an effort's target even if `--model` appears after the cursor. When entering a body command context, reuse the existing command/skill argument completion rules with an offset range; do not strip or replace the outer macro definition.

Add a bare-ID candidate kind whose Enter behavior submits the actual buffer. Tab/navigation may explicitly fill candidates; do not inherit the current slash popup's fill-and-immediately-submit path. Definition candidates always fill without submitting the definition. Candidate description previews are separate from the exact-match ghost hint. Keep `@` filesystem requests adapter-owned with their existing stale-result guards.

### One-row preview

Use a pure width-bounded projection of the matched body's first line. Its input is the complete stored body and an available terminal-cell budget; output is display text plus a truncation flag, never an executable body.

1. Compute raw input layout, visible rows, cursor/IME anchor, and row-end column without the macro body. Find the visible row containing the end of the real input, including trailing spaces. If it is offscreen, there is no hint to paint.
2. Reserve the normal separator/cursor cell between input and hint. Use only the remaining inner-row cells; do not shorten the typed ID or write into the rule/gutter to make room.
3. Iterate extended graphemes from the first logical line using existing Unicode width utilities. Render nonprinting controls safely for display without changing stored text. Detect CR/LF and other supported line separators as hidden continuation rather than flattening subsequent lines into the preview.
4. If all content fits and there is no continuation, display it unchanged. Otherwise reserve the display width of `…` and retain only complete graphemes that fit before it. Scan only as far as required to determine the visible prefix and whether more content exists; do not allocate the full expanded body every frame. Zero width yields no hint; a one-cell budget yields `…` when content is omitted.
5. Paint the hint after raw input layout, in `theme.activity.label.style()`, never append it to `InputState.buf` or wrapping/navigation input. Paint the real cursor last or reserve its cell so the hint cannot overwrite it. An optional visible-cell mouse copy remains visual only, not semantic prompt source.
6. Color the ID via `theme.coral`, preserving placeholder/cursor priority, and paint ` macro ` in the bottom rule using `input.status_hint`. The same exact lookup activates and clears all three. Use current viewport width and theme on resize/reload; never perform file or catalog I/O in rendering.

This deliberately differs from `display_with_model_hint`, whose displayed text can wrap. No color-specific test fixture or new styling theme field is needed.

## 5. Fixed decisions and discretion

User-confirmed requirements: exact bare-ID invocation with trailing spaces only; optional model/effort; real command/skill execution; full completion; Coral ID; lowercase bottom-rule `macro`; Umber inline expanded-body hint that is not editable; one-row preview with `…` for omitted content and complete execution payload.

Design choices fixed for this proposed package: shared user-level array persistence; ASCII identifier grammar; same-ID full replacement; independently optional effort; explicit `--` separator; single expansion and single-input semantics; Records preserve optional state until dispatch and allow steering replacement with one inherited baseline A; locally rejected contradictory/inapplicable command/route combinations; no implicit Enter execution of fuzzy candidates; existing semantic theme roles. These are proposed implementation decisions, not claims of delivered behavior or separately quoted user instructions.

Local discretion: private type/helper names, how the partial parser represents spans, extraction boundaries for controller helpers, and allocation/caching details that preserve the ownership and width bounds above. Do not broaden command support into a script engine or silently omit DSH/skill completion to simplify implementation.

There are no unresolved core design choices in this package. If a native completion limitation prevents the specified settlement boundary, report it and revise the design before implementation rather than weakening acceptance. Additional automated tests require separate user authorization; that is a verification permission boundary, not permission to skip functional checks or modify existing tests.

## 6. Verification and documentation impact

Use [S1-S7](specs/prompt-macros.md#acceptance-scenarios) as implementation acceptance, not as an instruction to author tests. Reuse existing scoped checks, including:

```text
cargo check --workspace
cargo test -p e-tui --lib input
cargo test -p e-tui --lib runtime::command
cargo test -p e-tui --lib runtime::controller::model_tests
cargo test -p e-tui --lib model_defaults
cargo test -p e-tui --lib config
cargo test -p e-pi --lib adapter::queue
cargo test -p e-dsh --test architecture
```

Select additional existing adapter tests only when their paths change. This is a cross-cutting Rust implementation, so finish with workspace formatting and `cargo clippy --all-targets` per the [testing guide](../../../../guides/testing.md). Do not run an unfiltered full Rust test suite by default. Do not add, expand, or edit tests, including snapshots/theme-styling tests, without explicit authorization. Preserve existing tests where APIs can remain compatible; if a necessary API change would require a test edit, request permission rather than quietly updating it.

Manual terminal checks must exercise exact vs fuzzy invocation, definition/cursor completion, all route combinations and restoration failures, skill execution on both adapters, and narrow/wide Unicode/multiline previews. Use harmless prompt bodies, not the example's actual commit request; isolate config files when exercising persistence errors. Do not claim an unavailable backend/model or untested terminal as verified. If bridge production changes become necessary, read its contracts/deployment guide first and add the required generated-asset and deployed-bridge checks; no bridge change is assumed by this design.

After delivery, update the interaction/presentation/configuration contracts listed in [Current-document impact](specs/prompt-macros.md#current-document-impact), plus one architecture ownership statement. Document any new cross-boundary settlement invariant in the runtime contract. Update the e-tui help/command hints without turning Help into a full command catalog. No root README change or new ADR is needed. Creation leaves all current contracts untouched; execution does not authorize Doco completion/archive.

## 7. Approved Record lifecycle revision

The user approved prompt-plus-optional-state Records, dispatch-time recovery capture, immediate steering replacement of consumed owners, and final restoration to A for A → B → C. This supersedes the original after-turn-only/exclusive-prompt design. Implementation uses a serialized authoritative state fence before first capture; successors inherit A, and per-Record admission/consumption latches prevent applying C to a still-queued B prompt. The original issue below remains verification context, not a pending design decision.

### Original boundary issue

- Original assumption: the shared idle/restoration barriers identify the execution window in which the latest authoritative return route can be captured.
- Observed boundary: `RuntimeState::is_fully_idle` checks agent work, compaction, and interruptible commands, but does not expose an in-flight ordinary model/effort selection. Pi serializes such requests behind `configuration_request` in the adapter. Routed macro preparation captures the original selection in shared state before its selection request reaches that serialized native window.
- Risk: an earlier manual selection can confirm after the macro has captured its return route. The body may select correctly while restoration returns to the older snapshot. This does not meet R4/S3/S5; cached idle or target equality cannot establish the missing operation boundary.
- Affected task: 2.2 now executes the approved revision and still needs cross-boundary verification; existing temporary-model tests alone do not establish this case.
- Decision: use the actual dispatch boundary and a serialized state fence, not frontend cached idle. Verify both adapter fences and the A → B → C → A sequence. No macro-specific wire field or test changes are authorized.
