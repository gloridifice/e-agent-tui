## Context

The client currently converts a small typed subset of DSH events directly into `Msg` variants and renders those variants through event-specific branches in `ui.rs`. This works for text chat, Thinking, generic tools, file-operation groups, notices, and the user card, but it does not provide a stable extension point for the broader DSH 0.1.0-rc.6 event vocabulary. The bridge also calls its whitelist a “surface” while merely filtering event types; neither side applies DSH `surfaceOp` replacement semantics.

The change crosses the bridge wire/history boundary, the Rust protocol anti-corruption layer, session projection, transcript cache, copy provenance, UI layout, and input handling. It must preserve the current performance constraints: streaming chunks cannot trigger full transcript reconstruction, redraw remains dirty-driven, visible-window cloning remains bounded, and input-accessory interaction must obey the main-loop lock discipline.

## Goals / Non-Goals

**Goals:**

- Establish four reusable presentation contracts: `ActivityRow`, `TranscriptBlock`, `ContentCard`, and `InputAccessory`.
- Route typed DSH events through one projection layer that emits display, transcript-mutation, page-state, accessory-state, or ignore effects.
- Preserve current visible behavior while migrating existing message types.
- Correctly apply append/replace surface semantics during live delivery, snapshot replay, and backward history paging.
- Add incremental support for todo, retry, command lifecycle, rich turn outcomes, context forms, nested Code Mode calls, and workflow activity.
- Keep rendering materialization and copy provenance outside the event reducer.
- Make unknown append-surface events visible through a bounded fallback without admitting raw JSON into application reducers.

**Non-Goals:**

- Reproduce every DSH Web UI visual treatment exactly.
- Add inline raster-image terminal protocols; image content receives a textual attachment presentation.
- Build a general-purpose tree widget or arbitrary plugin-renderer ABI.
- Display audit-only and reconstruction-only events in the ordinary transcript.
- Add horizontal scrolling or change existing Markdown atomic-block copy semantics.

## Decisions

### 1. Use owned display models, not renderer traits, as the public event-display boundary

The client will expose serializable/testable owned models conceptually shaped as:

```text
DisplayItem = Activity(ActivityRow) | Block(TranscriptBlock) | Card(ContentCard)
ProjectionEffect = Display(...) | Update(...) | SurfaceMutation(...) |
                   PageState(...) | AccessoryState(...) | Ignore
```

`InputAccessory` remains a separate UI model because it participates in focus, key handling, and bottom-page layout rather than transcript scrolling. Render functions consume these models; they do not receive `HostEvent`.

This is preferred over a trait-object widget registry because owned models are easier to compare in reducer tests, cache, clone only in the visible window, and project into copy provenance. A plugin renderer ABI remains out of scope.

### 2. Keep four surfaces composable rather than adding event-specific top-level variants

An activity row carries stable identity, label, summary, lifecycle state, optional timing, and optional `parent_id`/depth. A transcript block carries plain, Markdown-source, reasoning, or bounded-fallback content plus tone. A content card carries optional header, body content, padding/theme role, and copy source. Rich tools and compaction may compose an activity row with a content card instead of defining a fifth base surface.

Current file groups remain a specialized activity projection internally, but must satisfy the common activity contract and continue using the shared transcript layout for copy row provenance.

### 3. Introduce a typed event projector between protocol parsing and application state

`HostEvent::from_value` will parse event-level sequence, top-level time, `surfaceOp`, and source sequence metadata before constructing typed payload variants. The projector will maintain correlation indexes for tool calls, commands, retries, compactions, nested calls, workflows, and surface-node-to-display-item ownership. It emits small effects applied under one state guard.

This is preferred over continuing to grow `AppState::apply_host_event`, because correlation, semantic mutations, and presentation classification are separate responsibilities. Unknown payloads remain bounded metadata in `HostEventKind::Unknown`; `serde_json::Value` does not enter the reducer.

### 4. Apply DSH surface mutations before presentation

The projector owns ordered surface-node identity and a set of shadowed sequence numbers. For append operations it creates or updates the corresponding display item. For replace operations it removes display items owned by the replaced surface nodes before inserting the replacement at the replaced range’s position.

`sourceEventSeqs` is retained to recognize shadowed nodes that arrive later through backward history paging. Because paging proceeds newest-to-oldest, replacements learned from the initial snapshot can suppress subsequently loaded shadowed nodes. Tool activity ownership is linked to its `tool/result` surface node so replacing a tool result also removes the paired tool row.

Surface mutation is deliberately not represented as a visible display surface. Compaction lifecycle may still emit an activity row/card, but that presentation is separate from replacement correctness.

### 5. Expand history by semantic event family, while keeping one canonical wire contract

The bridge history roster will add only events needed to reconstruct supported displays or classify surface mutations, such as command lifecycle, compaction, retry, nested Code Mode, and workflow events. Realtime forwarding remains broad. Audit-only records stay out of snapshot/history unless they are required to interpret another included event.

Any roster/version change is made in `bridge/protocol-contract.json`, followed by generated protocol documentation and both bridge/Rust contract tests. Payload trimming remains in force; presentation metadata needed for a supported rich tool must be preserved explicitly rather than restoring unbounded results.

### 6. Give input accessories deterministic stacking and focus rules

Accessories are ordered by semantic priority and then stable insertion order. Blocking interactions such as approval and question receive focus ahead of informational accessories. Todo, goal/plan context, and queued prompts may coexist within the available budget; lower-priority informational rows collapse before the input editor is starved. Only the focused blocking accessory handles keys.

This extends the current bottom layout rather than rendering floating overlays. Every accessory render path must share one height calculation with the page layout and clear any covered area before painting.

### 7. Preserve presentation and cache boundaries

Markdown/reasoning/card body materialization remains outside the event reducer, following `presentation.rs`. Streaming text updates only the tail item and sets `tail_dirty`; activity state transitions and structural mutations invalidate the structural cache. Stable display IDs preserve unit ranges across rematerialization. Copy mode obtains rows from the same display-item layout path used by the visible transcript.

Activity animation continues to be dirty-driven. Correlation indexes replace reverse full-list scans where practical, especially for nested and long-running event families.

### 8. Classify non-display records explicitly

Session title, provider/model, request context, usage, and policy/mode selections update page or session state. `request/header`, `session/end-seed`, descriptor/audit/request records are ignored by transcript policy after any required reconstruction fields are consumed. The classification is exhaustive and tested so a newly typed event cannot silently acquire an arbitrary presentation.

Unknown events with append surface metadata use a bounded `TranscriptBlock` fallback. Unknown non-surface or replace events are retained for diagnostics but do not invent presentation semantics; an unsupported replace must fail safely rather than showing a transcript known to be incorrect.

## Risks / Trade-offs

- **[Risk] Surface replacements can reference nodes outside the loaded history window.** → Retain shadowed sequence metadata independently of loaded display items and test replacement followed by backward paging and repeated compaction.
- **[Risk] A single migration of all existing `Msg` variants could destabilize layout and copy mode.** → Migrate one surface family at a time behind equivalence tests, keeping stable IDs and the shared layout provenance path.
- **[Risk] Expanded history events increase snapshot size.** → Include only reconstruction-relevant families, retain caps/trimming, and add snapshot-size fixtures and bridge paging tests.
- **[Risk] Multiple input accessories can consume the transcript area or create ambiguous focus.** → Enforce a height budget, deterministic collapse order, and exactly one focused blocking accessory.
- **[Risk] Flat parent/depth activity rows are less expressive than the Web UI’s nested tool tree.** → Accept the compact terminal representation now; the stable parent identity leaves room for a future tree renderer.
- **[Risk] Unknown replacement semantics cannot be presented safely.** → Surface a protocol/compatibility error and avoid silently reconstructing an invalid transcript.
- **[Risk] Rich tool details may require payload currently trimmed by the bridge.** → Define bounded presentation projections or metadata per supported tool; never restore unrestricted tool output transfer.

## Migration Plan

1. Add the shared display models and projector effect types while adapting existing `Msg` rendering through a temporary compatibility conversion.
2. Migrate current user cards, assistant blocks, notices, Thinking, tool rows/file groups, and input accessories with UI equivalence tests.
3. Correct protocol parsing for top-level time, content blocks, result errors, and event-level surface metadata.
4. Implement append/replace projection and history paging semantics before enabling new compaction presentation.
5. Expand the canonical history roster and add todo, retry, command, turn-outcome, context, nested-tool, and workflow projections in bounded increments.
6. Remove the compatibility conversion and obsolete event-specific render branches after all existing behavior is represented by the four surfaces.
7. Regenerate protocol docs, run bridge and Rust suites, run live bridge smoke tests, and update project documentation.

Rollback is possible before step 6 by retaining the compatibility conversion. After the wire contract is bumped, an older bridge remains rejected through normal protocol negotiation rather than producing partially reconstructed history.

## Open Questions

- Whether reasoning blocks should default to expanded or collapsed in the terminal theme.
- The exact informational accessory collapse order among queued prompts, todo, goal, and plan mode after representative small-terminal snapshots are reviewed.
- Which tool-specific `meta` projections are sufficiently bounded to include in this change versus a later rich-tool presentation change.
