<!-- doco:change mode=proposal-only -->
# Center help in a Markdown modal

## Purpose

Detailed help currently has two inconsistent surfaces: the help action replaces message-pane content with plain styled rows, while the slash help command appends a large Markdown block to the transcript. This pollutes conversation presentation, confines shortcut help to one pane, and duplicates command discovery that already belongs to slash completion.

Unify both entry points as one centered, screen-level Markdown modal that preserves the underlying conversation and presents the effective key mapping clearly.

## Scope and acceptance

- The configured help action and the built-in slash help command open the same centered modal over the complete terminal screen.
- Opening help does not append a transcript item, send an agent request, alter the composer draft, or replace Preview content.
- The modal body is rendered through the shared Markdown renderer and shows the current effective key bindings in localized, user-facing groups without internal action identifiers.
- The modal does not enumerate built-in or runtime slash-command catalogs. It may direct users to slash completion for command discovery.
- The modal uses responsive centered geometry, keeps its title and navigation footer fixed, and supports mapped row/half-page/page navigation inherited from the full-screen scope plus mouse-wheel scrolling.
- The configured help action, the Help close binding, `Esc`, or `q` closes the modal according to the effective mapping. Reopening resets the modal to its first row and reflects reloaded configuration.
- Help remains a blocking input context while background agent state continues reducing. Existing page, approval, Reading, history, transcript, Preview, and composer state resumes unchanged after close.
- English and Simplified Chinese content remain equivalent.
- Current presentation and key-mapping documentation is updated for the modal surface and navigation behavior.

Non-goals:

- Changing slash-command completion or command catalogs.
- Changing the compact launch-time `--help` output of `dshe` or `pie`.
- Adding command search or links inside the modal.

## Result
Pending — not completed.
