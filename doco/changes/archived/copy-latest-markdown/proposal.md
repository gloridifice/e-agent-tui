<!-- doco:change mode=proposal-only -->
# Copy the latest Markdown response

## Purpose

Copying an assistant response currently requires entering Reading View or selecting rendered terminal cells. Add a direct slash command for retrieving the complete Markdown source without presentation wrapping, reveal state, or rendered styling.

## Scope and acceptance

- Add a `copy` slash command to the shared built-in command registry and localized slash completion for both frontends.
- Invoking the bare command copies the complete semantic Markdown source of the most recent completed assistant answer in the current transcript.
- Lookup ignores rendered text, reveal progress, reasoning, tool output, local notices, and user cards. A newer user message does not make the preceding completed answer unavailable.
- The command is frontend-local: it does not send an agent request, append a user message, or mutate transcript content.
- Clipboard access remains an adapter-owned `UiAction`, so existing success and failure feedback applies.
- Arguments are rejected with the standard local usage error. If no completed assistant Markdown answer exists, show a localized local error and do not write the clipboard.
- Focused tests cover exact source copying, most-recent selection, incomplete-answer exclusion, argument rejection, and the empty-transcript case.
- Update the current presentation contract for the new copy surface.

Non-goals:

- Copying reasoning, tool output, user messages, rendered terminal cells, or Preview content.
- Adding a key binding or changing Reading View and mouse-copy behavior.
- Forwarding the command to DSH or Pi.

## Result

Delivered. The shared built-in registry and localized slash completion expose `copy` to both
frontends. The frontend resolves the complete semantic Markdown source of the most recent
completed assistant answer, ignoring rendered text, reveal progress, reasoning, tool output,
local notices, and user cards, and dispatches the adapter-owned clipboard `UiAction` outside the
state lock. Arguments and a transcript without a completed assistant answer report localized
local errors without writing the clipboard.

Verification: focused command, controller, command-catalog, and localization tests passed
(`cargo test -p e-tui copy`: 25). The current presentation contract is synchronized. No live
terminal clipboard smoke run was performed.
