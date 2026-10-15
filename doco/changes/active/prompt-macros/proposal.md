# prompt-macros

## Purpose

Repeated prompts currently require retyping their body, selecting a model and effort, and remembering to restore the conversation route. Model-letter prefixes provide temporary routing but do not define reusable input or preserve every command-dispatch path when a body contains a slash command.

Add named input macros shared by `pie` and `dshe`, with exact bare-ID invocation, optional temporary routing, complete definition/invocation completion, and an inline indication of what will execute. The composer must make an exact macro invocation distinguishable from ordinary text without replacing the editable ID with its body.

## Scope and acceptance

### Definition and invocation

- <code>/prompt-macro &lt;macro_id&gt; [--model &lt;model_id&gt;] [--effort &lt;effort&gt;] &lt;prompt content&gt;</code> defines or updates one macro. Model and effort are independently optional. Definitions persist in e's shared frontend configuration, not backend configuration or key mappings.
- An invocation matches only when the entire original, text-only composer equals a registered, case-sensitive ID after removing trailing ASCII spaces. Leading characters or whitespace, extra text, tabs, newlines, and image attachments prevent invocation. Check before existing trailing-newline normalization.
- Expand once and classify the complete body through the existing input/command dispatch rules. Explicit <code>/skill:&lt;name&gt; &lt;message&gt;</code> and <code>/skill &lt;name&gt; &lt;message&gt;</code> must execute as skills with their accompanying message, not be sent as literal command text. Other single-input commands keep their existing ownership and admission rules.
- An override must be confirmed before dependent execution. Restore the original model and effort after authoritative settlement, including cancellation and failure; failed restoration blocks dependent work. Each message is an indivisible Record with optional model/effort. Capture the recovery baseline only at actual dispatch after prior controls settle. ASAP steering replaces the current consumed Record's temporary state while inheriting its original baseline A; A → B → C must finally restore A, never B.
- For the example <code>/prompt-macro cm --model openai-codex/gpt-6-luna --effort high 提交更改</code>, submitting `cm` sends the full body `提交更改` under the requested available route, then restores the prior route. The example does not assert that this model exists in every backend catalog.

### Completion

- Complete the command name, existing macro IDs, unused option names, model IDs, model-appropriate efforts, and the command/skill prefix inside the body. Retain existing `@` path completion in the body.
- Offer bare macro-ID candidates with a body summary and optional route information. Fuzzy candidate matching must never become fuzzy execution: Enter submits the actual composer, not an implicitly selected macro candidate.
- Completion must preserve nonedited suffixes, Unicode cursor positions, and atomic paste/image blocks. Esc restores the original completion query and cursor. Defining a macro never executes its body.

### Composer presentation

- Exact matching uses the same predicate for execution and presentation. Highlight the real macro ID in the theme's Coral color and place the literal lowercase `macro` label in the bottom composer rule, using the position, spacing, and status-hint style of the existing shell-command indicator.
- Show the expanded body after the real input as an Umber-equivalent, noneditable inline hint. Show body text only, including a leading skill/command invocation when present, not definition options or route metadata. Terminal font size remains unchanged.
- The hint must occupy only the remaining cells of that visual row. Never wrap or enlarge the composer for it. When width or a line break hides any body content, reserve space for one trailing `…`; truncate at grapheme boundaries using terminal display widths. If no hint cell fits, omit the hint rather than overwrite input, cursor, or borders.
- The hint must not enter the buffer, matching, editing/navigation, prompt history, submission, or semantic copy source. Rendering truncation must never truncate execution. Invalidating the match removes the Coral highlight, hint, and bottom label together.

### Boundaries and intended contract changes

This is a single-input alias feature, not a sequence of commands, recursive macro interpreter, argument/template language, or keybinding macro facility. Existing backend capabilities and command errors remain authoritative. Route overrides combined with commands that explicitly control routes, sessions, resources, or application exit must fail locally rather than establish contradictory lifetimes; those commands remain usable as ordinary no-override aliases.

The implementation will extend the current [interaction](../../../specs/interaction-and-sessions.md), [presentation](../../../specs/presentation.md), and [configuration](../../../specs/configuration-and-storage.md) contracts and the architecture's frontend ownership description. Adapter completion normalization must preserve the current [runtime boundary](../../../specs/runtime-and-adapters.md). Do not describe planned behavior as implemented in these documents during creation. No macro-specific DSH wire contract, Pi extension, root README change, or unrelated change-package work is included.

Verification must cover exact/nonmatching input, independent optional routing, skill dispatch, settlement/restoration, definition and invocation completion, and one-row Unicode-aware preview clipping. Existing scoped checks and manual exercises are planned; adding or modifying tests requires separate user authorization, and theme-styling tests are out of scope.

## Result

Implementation is in progress, including the approved Record lifecycle revision. Existing checks and remaining native/deployment acceptance gaps are recorded in [tasks](work/tasks.md). This change has not been completed or archived.
