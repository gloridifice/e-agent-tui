# History usage timeline

## Purpose

Execution history currently records turn boundaries and operation timing only.
It cannot answer which model produced a response, how many tokens a session or
model consumed, or what provider-reported price was attached to that usage. The
history page also exposes only the longest-operation ranking, so chronological
turn structure is not visible.

## Scope and acceptance

- Extend the append-only execution-history event schema with provider-neutral
  model selection, message-kind, and model-response usage records. Usage stores
  disjoint input, output, cache-read, and cache-write token counts plus an
  optional provider-reported USD amount in integer nanodollars.
- A history turn begins at a normalized user message and ends at normalized
  agent stop (`TurnEnd`). Reasoning/model steps, assistant messages, tool calls,
  tool results, and model changes are message events inside that turn; they do
  not independently create turns.
- Associate each recorded usage sample with the exact provider/model route
  observed when that response completed. Pi records native per-response
  `usage.cost.total`; DSH records token usage and leaves price unknown because
  its current host contract exposes no price.
- The query used when opening the history page returns chronological records as
  well as the existing top-50 call ranking under one finite watermark.
- The history page has two views. The existing operation ranking remains the
  initial view. The new timeline uses the approved Ferra prototype: session and
  per-model token/price summaries scroll with the content; a vertical fixed
  scale of five seconds per row shows model at left, message kind at center, and
  whole-turn token/price totals at agent stop on the right. It has no title,
  legend, column-heading block, row cursor/highlight, or bottom information bar.
- While history is open, the semantic `history.toggle_view` action defaults to
  `Tab` and switches views. Each view retains its own scroll offset. Existing
  movement and exit actions remain available.
- Unknown price is never displayed or aggregated as zero. A subtotal with at
  least one unavailable price is marked as partial; a completely unavailable
  price is shown as unknown.
- History records continue excluding model text, reasoning text, tool output,
  patches, file bodies, and raw unknown payloads. Existing version-1 files stay
  readable; new event variants are additive to the same envelope version.
- Update current presentation, interaction, and storage contracts for the new
  view, key action, turn boundary, and persistent fields.

Non-goals: pricing estimation, a built-in price catalog, changing provider
billing, changing transcript rendering, importing native historical messages
that were never observed by e, or replacing the operation-ranking view.

## Result

Pending — not completed.
