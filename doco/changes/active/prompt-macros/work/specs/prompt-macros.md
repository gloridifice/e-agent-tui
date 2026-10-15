# Prompt macro target contract

## Scope and current contracts

This is a planned contract, not current behavior. It extends the [interaction](../../../../../specs/interaction-and-sessions.md), [presentation](../../../../../specs/presentation.md), and [configuration](../../../../../specs/configuration-and-storage.md) contracts while retaining the [runtime and adapter boundary](../../../../../specs/runtime-and-adapters.md).

A macro represents one complete input submission. No recursive expansion, positional arguments, command chains, automatic execution on matching, new key bindings, or macro management page is included. Definition/update is the only new management command; file edits and <code>/reload</code> remain available for removing definitions.

## Requirements

### R1. Definition and storage

The command is <code>/prompt-macro &lt;id&gt; [--model &lt;reference&gt;] [--effort &lt;effort&gt;] &lt;body&gt;</code>. Option order is independent. Header options stop at the first body token; subsequent option-looking text belongs to the body. A standalone `--` ends header parsing explicitly. Preserve body text from its start, including internal spacing, command syntax, trailing whitespace, and subsequent lines; do not tokenize and rejoin it.

IDs follow `[A-Za-z][A-Za-z0-9_-]*` and are case-sensitive. A definition requires a non-whitespace body and text-only input. Reusing an ID replaces its entire definition, including clearing previously specified options that are now omitted. Duplicate options, missing values, malformed IDs, and unknown header options are local errors; the old definition stays intact and the rejected input remains recoverable.

Store `prompt_macros` as a validated array in shared `config.toml`, with entries containing `id`, `prompt`, optional canonical `model`, and optional `effort`. Default to an empty array. Resolve explicit models using the existing canonical-or-unique-bare-ID policy. With an explicit model, validate the effort against that model at definition and invocation. Without a model, retain the effort identifier and validate it against the invocation route; defining it must not permanently bind it to the definition-time model.

Reload updates future matching, hints, and candidates. Unavailable saved routes remain stored but fail locally when invoked. Configuration writes remain adapter-owned. A write failure must be visible and must not be reported as a successful save; if the new definition is already live, identify it as unsaved. Macro definitions never write Pi/DSH configuration or change model-default preferences.

### R2. Exact invocation and expansion

Use the original composer buffer, before trailing-newline normalization, and require no image attachment. Match `raw.trim_end_matches(' ')` against the registry. Only trailing U+0020 spaces are ignorable; no case folding, leading trim, substring/prefix matching, or general whitespace trim is allowed.

For a registered `cm`, `cm` and `cm   ` match; `c`, `CM`, ` cm`, `cm x`, `cm\t`, `cm\n`, `cm\r`, `cm\u{00a0}`, and <code>//a cm</code> do not. Nonmatches follow the existing normal input policy. A paste placeholder's complete underlying text is authoritative, not its displayed placeholder label.

At submission, capture the selected definition and resolved route into an owned invocation snapshot. Expand only the top-level ID and classify the body as one existing input action. A body equal to another ID is not recursively expanded. The recalled composer history contains the user's original invocation; admitted native prompt/history and transcript content reflect the actual expanded submission under the existing prompt/skill/command rules.

### R3. Command and skill execution

Do not send all expanded bodies through `AgentRequest::Input`. A leading slash command must use the existing local/builtin or adapter command dispatcher, and a leading `!` must retain local-shell semantics. A command appearing in the middle of ordinary text or on a later line is not separately executed.

Both skill forms execute with their entire accompanying message. Pi must retain its one-native-prompt skill/message behavior. DSH must use its native skill injection path rather than submit raw <code>/skill:...</code> as ordinary user text. The same applies to the first eligible submission after a bare <code>/new</code> draft.

No-override command aliases retain normal command ownership, restrictions, errors, and session lifecycle. With an override, ordinary prompts, explicit skill invocations, and backend-executed commands use the temporary-route barrier. Reject known contradictory controls before any effect: <code>/model</code>, <code>/effort</code>, <code>/new</code>, <code>/resume</code>, native fork/clone, <code>/compact</code>, <code>/reload</code>, authentication/MCP controls, exit commands, or a second <code>//</code> model prefix. Local UI/config/copy/shell-only commands also reject route options as inapplicable instead of silently ignoring them. Their no-override aliases remain supported. Unknown commands retain the backend's normal unknown-command behavior and are never reinterpreted as plain model messages by the frontend.

### R4. Temporary route and scheduling

| Definition | Resolved invocation behavior |
| --- | --- |
| Neither option | Use the conversation baseline; when superseding a temporary Record, do not inherit its temporary target. |
| Model only | Select that model with its supported e-configured default effort, otherwise the existing backend model-selection behavior. |
| Effort only | Resolve the model from the authoritative baseline at dispatch and change only its effort temporarily. |
| Both | Select the resolved model and explicitly supported effort. |

A message Record owns its complete prompt and `PromptState { model: Option, effort: Option }`. Capture the body, explicit model, effort, and any applicable configured model default at admission; do not capture a return route or bind an omitted model there. Queued records remain indivisible. Definition/default edits cannot retarget them.

Records follow the requested ASAP/after-turn policy, with live compaction forcing after-turn. At the actual dispatch window, first settle prior model/effort controls, revalidate the requested state, and only then capture the authoritative baseline A. Confirm the target and requested effort before body dispatch. A mismatched effort fails selection. Records that are still waiting in the native queue MUST retain their route until consumption; do not apply a later record's state to an earlier queued prompt.

An eligible steering Record supersedes the current consumed Record without waiting for whole-run idle. Retire the old state owner, inherit A, and apply the new target C; never capture temporary B as a return route or restore B between records. Ordinary steering without overrides uses A. Only the latest owner may restore A after authoritative settlement. Older admission, command, or completion events MUST NOT restore or release its successor. Conflicting manual native mutations remain rejected during ownership; local navigation and cancellation remain available.

Native prompt acceptance is not agent-run completion. No-run commands require their own settled result/state; a pre-dispatch idle snapshot cannot release a Record. Records awaiting admission/consumption and unsettled commands cannot be superseded by another native submission.

Restore the exact prior model and effort after completion, rejection, or interruption. Do not apply saved default efforts during restoration. Selection failure must not dispatch the body; partial selection still requires restoration. Failed restoration keeps dependent work blocked and reports a recoverable error. Do not automatically replay commands after an uncertain outcome. Session replacement invalidates the old invocation and its callbacks; never restore an old session's route into an unrelated replacement. Expected materialization of a <code>/new</code> draft transfers the invocation owner using the existing new-input handshake.

### R5. Completion

Complete the following from in-memory configuration and live adapter catalogs:

- <code>/prompt-macro</code> in the built-in command roster;
- existing IDs in the definition's ID position, without preventing entry of a new ID;
- unused `--model`, `--effort`, and an explicit header/body separator;
- canonical model routes in the model value position;
- supported effort values for the explicit target model, or current model when none is supplied;
- the leading command/skill and its already-supported argument positions inside the body;
- existing `@` path requests inside the body;
- bare macro IDs at the start of an otherwise eligible single-token composer, with body summaries and route/effort descriptions.

The parser must support partial input and cursor-local replacement ranges. When the cursor revisits an effort option and a model option follows it, use the complete parsed header to determine the target. Invalid/unresolved explicit models must not borrow another model's effort list.

Fuzzy matching ranks candidates only. Bare-ID completion navigation/Tab can fill an ID; Enter sends the real buffer and cannot silently fill and execute the selected fuzzy candidate. A typed exact ID remains invocable while its candidate popup is open. In macro definitions, accepting an option/model/effort/embedded-command suggestion fills and dismisses without saving the definition or executing its body. A later explicit submission defines it. Existing ordinary slash-command completion behavior remains unchanged.

Replace only the active range, preserve the header/body suffix and atomic blocks, and restore the original query and cursor on Esc. Candidate lists refresh after successful definition changes, reloads, and relevant backend catalog changes. Disabled/remapped semantic completion actions retain their current meaning; no new fixed key path is introduced.

### R6. Composer feedback

A registered exact invocation, not a fuzzy candidate or a definition command, activates all three indicators together:

1. The real ID uses `theme.coral`, with existing cursor/placeholder styles taking precedence.
2. The bottom rule reads literal lowercase `macro`, using the existing shell-status label position, padding, and `input.status_hint` style; no extra row is allocated.
3. The body follows the real input as a noneditable hint in the existing Umber-equivalent activity-label tone. It displays the literal body, including a leading skill/command, but not definition options or model/effort metadata. It is not Markdown or ANSI execution and does not change font size.

Compute input wrapping, composer height, scrolling, and the software cursor/IME anchor from real input only. Place the hint in the remaining cells after the last real-input cell on that same visual row, reserving the normal cursor/separator space. It must not wrap, displace real text, increase composer height, or make a new editable/clickable cursor position. If trailing spaces or a narrow viewport leave no room, retain the ID/label but omit the hint.

Use only the body's first logical line. Omission of a following line counts as truncation even when the first line fits. If any content is omitted, fit one trailing U+2026 `…` inside the available hint width, clipping at extended-grapheme boundaries with terminal-cell widths. When truncation is necessary and only one hint cell is available, show only `…`; zero cells show nothing. Control characters must not emit terminal controls or create additional rows; render them safely for this preview only. Resize recomputes the clipped hint from the complete immutable body.

The hint never changes the real buffer, matching, movement, deletion, recalled prompt, dispatched content, semantic source/copy, or route. Existing visual mouse selection may copy displayed hint cells as visible text; it must not fabricate the hidden full body. Search/page/approval contexts keep their existing ownership. When input no longer exactly matches, clear all macro feedback in the next frame. A stale/unavailable target still identifies a registered macro; invocation reports the routing error rather than silently submitting its ID.

## Boundaries, errors and compatibility

- Definition errors are local, cause no backend submission, and do not mutate the previous definition. Unsupported invocation routes/efforts/command combinations also fail before body dispatch.
- Keep the original ID input recoverable on failed admission. If the composer contains newer text, retain the failed invocation visibly instead of overwriting that text. Do not silently retry an expanded command.
- Default configuration, theme roles, command registries, queue semantics, and adapter execution remain under their existing owners. No macro-specific wire protocol or native extension is introduced.
- Prompt macros are distinct from the currently unsupported keybinding/multikey macros. Do not relax that key-mapping restriction.
- A route-bearing invocation cannot bypass approval, authentication, MCP, compaction, session, or native-command guards just because it originated in a macro.

## Acceptance scenarios

### S1. Definition and persistence

Define the example macro, restart/reload the frontend, and inspect/invoke it. The full body and canonical optional route survive. Redefine without options and verify the old overrides are removed. Invalid/duplicate options preserve the prior definition; a failed write produces an unsaved/error notice, not a saved confirmation. Load an older config with no macro array and retain normal behavior.

### S2. Exact input

Exercise every match/nonmatch in R2, including a real trailing newline before commit normalization, pasted input, and an image-bearing draft. Only exact text-only invocations expand. A body naming another macro is submitted once as literal body text. No matching/highlighting event alone starts work.

### S3. Route combinations and isolation

Exercise neither/model/effort/both options against available catalogs. Observe confirmation before body dispatch and exact restoration afterward. Queue an after-turn macro during work/compaction and verify no early route change. For ASAP steering, exercise A → Record₁(B) → Record₂(C) → A, including an unconfigured Record₂ returning to A instead of inheriting B. Verify pending native Record₁ is consumed before C is applied, retired completions cannot restore B/A, and prior manual selections settle before A is captured. Redefine the macro/default while queued; captured explicit options stay unchanged and an omitted model resolves only at dispatch.

### S4. Commands and drafts

Invoke both skill syntaxes with a message, including as the first input after <code>/new</code>; observe skill execution and the complete message, not raw command text or a second Pi steering message. Exercise a no-override local command and a backend command that starts no agent run. Check that a conflicting control plus route options fails locally without partial execution. Command errors must not fall back to model text.

### S5. Failure, cancellation, and stale ownership

Observe unavailable model, unsupported effort, rejected model/effort selection, admission rejection, interruption, restoration failure, and session replacement. No body dispatch follows failed selection, newer drafts are preserved, prior routes are restored when still owned, and stale results cannot mutate a replacement session or release its queues.

### S6. Completion without accidental execution

Complete IDs/options/models/efforts and an embedded skill with text following the cursor. Verify effort candidates follow the target even when its option occurs later. Preserve suffix/paste blocks and restore the query on Esc. With only `c` typed and `cm` selected in suggestions, Enter submits `c`, not the macro; after explicitly filling `cm`, Enter invokes it. Accepting definition suggestions does not save or run anything. Existing slash and `@` completion remain usable.

### S7. One-row feedback and Unicode

With `cm` and `cm   `, observe Coral ID, bottom `macro`, and an Umber body hint without body/options entering the input. Exercise CJK text, combining marks, emoji ZWJ sequences, multiline bodies, exact fit, overflow, one-cell and zero-cell budgets, and narrow/wide resize. Truncated previews end in `…`, never wrap or split a grapheme, and execution still receives the full original body. Move the cursor, use Home/End, and invalidate the match; editable positions remain unchanged and stale feedback disappears.

## Current-document impact

After delivery, synchronize macro definition/invocation/completion and routing into the interaction contract, the three-part composer feedback into presentation, and the array/persistence policy into configuration. Clarify that the key-mapping macro restriction is unchanged. Update architecture for frontend macro ownership and runtime contracts only for any newly introduced normalized settlement invariant. Preserve the root README and unrelated documentation.
