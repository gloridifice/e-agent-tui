## Why

The read-only `e-tui` audit found duplicated presentation and input policies, oversized mixed-responsibility modules, and a Main-pane dependency back into its composition root. The existing architecture tests pass but miss local module paths and grouped imports containing aliases, so their current result does not establish an acyclic implementation.

## What Changes

- Repair architecture dependency discovery and its regression fixtures, then move Main composition and Region implementations to their owning rendering layers. Move page-shell geometry out of transcript rendering and common ruled chrome into a Component; preserve the adapter-facing rendering entry points.
- Centralize completion ranking mechanics and candidate construction without merging distinct search fields or tie-breaking rules. Group each suggestion's fill, description, and source into a row value where compatible with supported callers.
- Reuse atomic input-range maintenance and linear focus-node construction through concrete values or statically dispatched helpers. Keep paste/image storage separate and preserve page-specific navigation edges.
- Share bottom-area measurement and accessory allocation between viewport calculation and drawing. Keep Input Page positioning distinct from the ordinary scrollable bottom stack, including deferred-new accessory suppression.
- Isolate width-clipping corrections from mechanical refactors: retain exact-fit text, preserve complete graphemes including those crossing style spans, and keep plain clipping separate from ellipsis policy. Preview terminal output must remain one row per source row with no synthetic ellipsis.
- Reuse RGB interpolation, breathing phase, ruled-line styling, transcript RenderOptions construction, and spinner eligibility. Share fade-group bookkeeping while retaining distinct transcript admission and Preview row/block state machines.
- Split the production responsibilities in `input`, `input_page`, `render`, `ui/transcript`, and `runtime/state/reduction` into private submodules. Isolate legacy test mirrors before assessing remaining production size; retain their regression protection rather than deleting them as apparent duplicate production paths.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change restores and preserves existing requirements; it does not introduce new interaction, protocol, persistence, rendering-surface, or scheduling contracts.

## Impact

- Primary scope: `crates/e-tui/src/`. Cross-package scope is limited to `crates/e-dsh/tests/architecture.rs`, necessary test-only scanner dependencies, and adapter/example import compatibility. No bridge or provider lifecycle redesign. No new runtime dependency or generic page/render/cache framework is planned.
- Use the repository's Lite workflow with `skip_specs: true`. Checked main capabilities: `acyclic-client-architecture`, `input-page`, `event-display-surfaces`, `terminal-render-performance`, `paced-text-reveal`, `testable-runtime-ports`, and `turn-model-prefix`. The existing acyclic/downward boundaries, shared accessory height calculation, width-aware layout, semantic-copy ownership, input preservation, and independent reveal clocks remain unchanged. Helper names and file boundaries are implementation decisions, not new specification requirements.
- Also checked relevant active deltas in `move-runtime-into-e-tui` (package/runtime boundaries), `refine-preview-and-markdown-rendering` (completion-gated folding, Preview row/block reveal, terminal clipping), `add-configurable-key-mapping` and `add-ui-localization` (page navigation and identity), `add-image-paste-input` (atomic prompt content), `enable-screen-wide-mouse-copy` (committed-screen selection), and `add-draggable-pane-separator` (bounded placeholder frames). These existing changes own their features; this change owns only cross-cutting audit remediation and does not duplicate their delivery or archive them.
- Specification drift is visible: main `paced-text-reveal` still describes row pacing for every Ready Preview, while the implemented active delta distinguishes fresh reasoning from block/page fade. Main `input-page` also retains older roster/category wording; current architecture and implementation use Resume/Question pages and display-only Settings categories. Preserve the characterized current behavior and agreed deltas; do not revive obsolete behavior, treat a draft as approval, or silently rewrite unrelated specifications. If a touched behavior cannot be reconciled with agreed requirements, pause that task for an explicit decision.
- The audit used the current dirty working tree, including temporary-model prompt work. Preserve those changes and their tests. Keep public adapter-facing paths stable with downward re-exports where appropriate; do not remove public compatibility paths merely because repository callers are few. Any newly necessary public contract change must first remove `skip_specs` and add the affected delta.
- "Zero-cost" means no additional dynamic dispatch, allocation layer, payload cloning, or worse asymptotic work solely to support an abstraction. Keep current storage layouts where practical; use borrowed boundaries and static helpers. It is not a claim of identical machine code or an unmeasured frame-time improvement.
- Highest-risk seams are Unicode clipping, scanner false negatives/positives, cache invalidation and history anchors, source-versus-visible copy, and independently scheduled reveal clocks. Use focused regressions and before/after workload counters; separate behavioral fixes from file moves. See [client architecture](../../../docs/subsystem/client/architecture.md), [testing policy](../../../docs/testing.md), and [performance methodology](../../../docs/subsystem/performance/methodology.md). Internal moves alone do not require documentation edits.
