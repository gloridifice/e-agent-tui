# User message Markdown and folding

## Purpose

Sent user messages currently display literal Markdown and can occupy an unbounded number of transcript rows. Render their Markdown consistently with assistant content and keep long prompts compact without losing the original message.

## Scope and acceptance

- Apply to sent user messages in the shared message pane, including live submissions, replayed history, and user messages carrying attachment labels. Preserve their existing shell, padding, and ordinary-text tone.
- Reuse the assistant Markdown renderer for inline formatting, lists, quotes, tables, code, and configured Mermaid rendering. Parse the complete message before folding.
- Count body rows after Markdown layout and width-aware wrapping, excluding the outer shell and optional card header. At most 20 rows remain complete; above 20 show the first 10 rows, one `...(<n> lines)` row, and the last 10 rows. `n` is the number of hidden rendered rows. Markdown structural rows participate in this count.
- Recalculate layout and folding when the content width changes. Retained rows preserve styles and Unicode graphemes. The marker is clipped rather than wrapped if the pane is too narrow.
- Fold tables and code only as part of the enclosing user message. Other Markdown surfaces retain all table/code rows.
- Keep the message pane folded in both normal and Reading modes. Existing Preview selection displays the complete Markdown message; add no expand/collapse action.
- Preserve sent payloads, semantic transcript content, history, and whole-message copy. Mouse selection continues to copy only visible cells.
- Do not change the composer, attachments themselves, skill/context presentation, assistant rendering, adapters, configuration, or key bindings. Do not add or modify tests.
- Update the current presentation contract to document the delivered behavior and the user-message exception to the no-block-folding rule. Validate with scoped existing tests and a frontend build check.

## Result

Pending — execution authorized; completion and archive are not requested.
