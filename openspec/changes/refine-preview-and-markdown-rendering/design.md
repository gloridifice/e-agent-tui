## Context

All three issues affect presentation state in `e-tui`, but the complete semantic transcript and Preview content must remain unchanged for Reading View, copy, replay, and adapter neutrality. Today the transcript cache renders every activity node, Preview flattens every content kind into one uniformly wrapped line list, and `LineRevealTrack` row-paces every Ready Preview. Live Markdown reveal also reconciles against rendered text; a changing generated code-block header such as `N lines` can move its common prefix backward and repaint early rows.

The issue text contains one conflict about terminal output overflow. This design follows the more specific final bullet: tool information wraps, while tool output is `nowrap` and receives no added ellipsis.

## Goals / Non-Goals

**Goals:**

- Bound long normal-mode runs of one-row activities without deleting semantic nodes or changing Reading View.
- Preserve tool identity and primary information while a long terminal secondary grows.
- Apply reveal granularity according to content origin and kind rather than row-pacing every Preview.
- Prevent append-only code-block streaming from moving the transcript reveal frontier backward because generated Markdown chrome changed.
- Keep cache invalidation and visible-row materialization bounded.

**Non-Goals:**

- Truncate stored tool output, copy sources, or Reading content.
- Add scrolling/keybindings, configuration fields, provider-specific logic, or protocol changes.
- Change assistant reply character pacing outside the code-block stability fix.
- Change non-terminal structured tool layouts that fit normally.

## Decisions

1. **Compute an ephemeral, boundary-gated activity-fold plan in the transcript layout path.** A linear plan groups consecutive collapsible one-row activity nodes after existing hidden-reasoning filtering, but keeps the live trailing activity phase expanded. It commits eligible runs only when assistant Markdown or an interruption/error outcome closes the phase. Rich activity nodes with informational detail separate one-row runs but do not commit them; unrelated ordinary content abandons pending candidates rather than triggering a fold. Completed runs of at most six remain unchanged; longer runs render nodes 1-3, one synthetic localized `... (N lines)` row, and the final three. Hidden nodes keep no cache range, and the synthetic row owns no semantic copy unit. Reading mode disables this plan and invalidates the transcript cache on entry/exit so the same semantic store derives complete Reading geometry. This avoids mutating `TranscriptStore` or creating a fifth display surface.

2. **Keep Preview section geometry beside the cached styled rows.** Preview layout materialization records the wrapped tool-information row count and the first terminal-output row. Tool name and primary content use ordinary wrapping. Each terminal secondary source row remains one cached row through reveal reconciliation, then is display-width clipped without a marker at the final viewport boundary before selection registration and painting. The generic wrapper never receives terminal rows. When all rows fit, existing vertical centering remains. Once content overflows, the information rows are pinned at the top and the remaining viewport shows the newest available terminal-output tail below them.

3. **Represent Preview reveal intent separately from semantic content.** Target selection records whether the target is a fresh live target or a page-style presentation (replay, cached revisit, or Reading selection). A fresh live `Reasoning` target continues to use row pacing. Every non-reasoning target is admitted as one block; page-style targets admit the complete page as one fade group regardless of content kind. Same-target revisions retain their common visible prefix and admit newly introduced content as one block, so settlement does not replay the tool header.

4. **Extend the existing reveal track rather than adding a second scheduler lane.** `LineRevealTrack` accepts row-paced or whole-block admission while preserving the current independent fade deadline, grapheme-safe style application, width reconciliation, and zero-rate behavior. The Preview pane remains the sole owner of the track and one deadline.

5. **Treat append-only live Markdown rematerialization as a monotonic reveal frontier.** Transcript reconciliation will preserve the already painted frontier when an append-only assistant source causes generated code-block chrome or syntax layout to change before that frontier. It still recomputes admission from the current rendered layout and clips to the new signature length. Non-append replacement and width/theme rematerialization retain common-prefix reconciliation. This prevents generated `N lines` metadata from replaying early code rows without exposing unrevealed tail content.

6. **Preserve terminal transparency for separator roles with no background.** The Screen always paints the separator glyph and semantic foreground, but resolves an omitted `bar.bg` or `line.bg` to terminal `Reset`, not `theme.bg`. An explicitly configured separator background remains authoritative, and drag placeholder fills retain their separate `placeholder.bg` role.

## Risks / Trade-offs

- **A pinned information section can consume the entire short Preview.** → Render information first and show output only in remaining rows; never hide tool identity to force output visibility.
- **Hard clipping drops off-screen terminal columns.** → Preserve complete output in semantic content/copy paths and follow the issue's explicit no-wrap/no-ellipsis presentation rule.
- **Mode-dependent transcript rows change viewport height.** → Invalidate and rebuild on Reading entry/exit, then use the existing display-row anchoring and Reading derivation paths.
- **Monotonic reveal across generated-layout changes is less exact than source-to-render provenance mapping.** → Limit it to append-only live assistant updates and cover changing code-block line counts; replacement and resize paths keep strict common-prefix behavior.
- **Synthetic fold rows have no semantic owner.** → Exclude them from Reading/copy provenance by design and test that Reading mode exposes every original activity.
- **A trailing run can occupy many rows while tools are still active.** → Defer folding deliberately so live work does not jump; commit only at Markdown or interruption/error boundaries.
