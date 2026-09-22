# Presentation contracts

## Event surfaces

- Every visible normalized event MUST project through `ActivityRow`, `TranscriptBlock`, `ContentCard`, or `InputAccessory` before rendering.
- `TimelineModel` MUST own the sole transcript store and projector. Production render paths MUST NOT add provider/event-specific top-level surfaces.
- Surface replacement MUST remove the shadowed owner and insert the replacement at its original position. History prepend MUST NOT revive shadowed content or regress newer page state.
- Unknown compatible events MAY render a bounded fallback; unknown raw payloads MUST NOT cross the adapter boundary.
- Pi fallback recovery MUST update one correlated message-area activity with attempt count and a seconds-resolution countdown, then explicit running, success, cancellation, or exhausted/rejected status. Partial assistant output MUST NOT mark this activity successful. Countdown updates MUST NOT append rows or accumulate surface sequence ownership.

## Layout and cache

- Layout width is the resolved pane content width. Wrapping and clipping MUST operate on Unicode grapheme clusters and MUST NOT split combining sequences or emoji ZWJ clusters.
- Wrapping MUST preserve UAX #14 break opportunities, keep glued punctuation from starting rows, and fall back to grapheme splitting only for over-wide atoms.
- Structural changes may rebuild layout. Streaming and paced reveal MUST splice only the affected suffix; spinner frames MUST patch active ranges. Reading and Preview navigation MUST NOT flatten or rebuild transcript semantics.
- Markdown inline styling MUST consume parser events, not reparse flattened text. Tables, lists, quotes, code blocks, and provenance MUST share width-aware layout rules.
- Markdown tables and code blocks, including rendered Mermaid diagrams, MUST retain every row regardless of block length; presentation MUST NOT replace middle rows with a collapsed head/tail window.

## Reveal and Preview

- Semantic transcript and Preview content MUST remain complete; reveal progress is presentation-only.
- Replay/history and copy operations MUST use complete semantic source. Plain-color mode may disable interpolation but MUST preserve pacing behavior.
- Preview selection, scroll, layout cache, and reveal progress are independent of transcript state. Stale async results MUST be rejected by target identity and revision.
- Tool Preview MUST use provider-neutral seeds. File mutation previews use event-supplied fragments; the frontend MUST NOT read files or compute missing diffs.

## Execution-history timeline

- The chronological history view uses a fixed vertical scale of five seconds per row and MUST preserve elapsed gaps. Events sharing a row remain message blocks inside their enclosing user-to-agent-stop turn; they MUST NOT become independent turns or duplicate usage.
- Total and per-model token/price summaries are part of the scrollable document. Known native prices are summed; any subtotal containing unpriced token usage is marked partial or unknown rather than treating the missing amount as zero. Agent-stop rows show whole-turn usage totals; open turns have no stop total.
- The timeline uses semantic theme roles and has no title, explanatory legend, column-heading block, row cursor/highlight, sticky section, or bottom detail/key bar. The operation ranking retains its existing presentation.

## Screen and copy

- Rendering layers point downward as Screen to Pane to Region to Component. Provider-specific rendering is forbidden.
- Running Thinking indicator labels MUST use a single left-to-right highlight sweep followed by a dim pause, interpolating from the theme's activity-label tone to its activity-detail tone (Umber to Bark in Ferra). Settled labels and reasoning content MUST NOT receive this effect; spinner and count styling remain unchanged.
- The composer and its replacement Input Pages show the frontend indicator, route, and reasoning-effort value in the top rule. Effort has no prefix and reserves width before route clipping; working animation, temporary-model italics, and model/effort selection feedback remain intact. Cache hit rate, context usage, cost, and help stay in the footer, with title and workspace below.
- The composer grows and shrinks with explicit and wrapped lines up to its row cap, then keeps the cursor's wrapped row in view without changing multiline editing, atomic paste, or submission semantics. Input Pages retain their own height policy.
- Recognized slash-command names in the composer use the theme's Blush-equivalent tone, including built-ins, exact runtime catalog names, and discovered `/skill:<name>` invocations. Arguments, unknown names, and ordinary text retain the normal input tone; placeholder and cursor styles take precedence.
- Detailed help MUST use one centered screen-level Markdown modal for both the help action and the built-in slash help command. Opening help MUST NOT append transcript content or replace Preview content, and the modal MUST show resolved key bindings without enumerating built-in or runtime command catalogs.
- Visual mouse selection copies only committed visible cells, remains inside its starting pane, preserves graphemes, and excludes separators. Reading copy uses complete owning-block source instead.
- The built-in `copy` slash command MUST copy the complete semantic Markdown source of the most recent completed assistant answer in the current transcript through an adapter-owned clipboard effect. It MUST ignore presentation state, reasoning, tools, local notices, and user cards; it MUST remain local and reject arguments. When no eligible answer exists, it MUST report a local error without writing the clipboard.
- Quick-link discovery from the latest completed assistant Markdown MUST remain pure and bounded. URI, absolute-path, and workspace-relative targets MUST be classified explicitly. Chinese/full-width opening wrappers are soft local-path boundaries: discovery retains longest-first hypotheses, validation selects at most one existing interpretation, and no interpretation is retained when an ambiguous group has no existing path. Selected targets are deduplicated before the first 36 receive presentation-only tags; overlapping rendered matches use the longest selected target, while semantic and copied source remains unchanged.
- Selection holds an immutable presented snapshot while background reduction continues; cancellation or release restores live rendering without losing dirty work.
- Authentication secrets and manual callback values MUST render only as masks and MUST be absent from copy surfaces, transcript/history, errors, diagnostics, and provider catalogs. Authorization URLs, device codes, provider labels, and native guidance are displayable metadata; URLs and device codes expose explicit page-local open/copy actions and MUST NOT enter transcript notifications.
- Resume pickers with adapter-supplied parent relationships MUST show selectable parent-first trees with connectors and a `(fork)` marker on derived sessions. Families are ordered by their newest member; nested descendants retain their hierarchy. Search matches titles/IDs and retains available ancestors. Missing parents remain visible roots, malformed cycles MUST terminate without losing entries, and ages remain right-aligned without wrapping. Pickers without ancestry remain flat.
- Model menus show an e-configured default effort after the model name and before any letter mark. The annotation uses the subdued activity-label tone (Umber in Ferra), reserves display width before name clipping, and reads only in-memory config.
- Pi automatic compaction MUST display `auto compacting with default model` while running and `auto compacting complete with default model` after success. It MUST NOT infer an extension-selected compaction model. Manual compaction and DSH retain their model-aware labels.
- The terminal hardware cursor MUST remain hidden; the TUI uses a software cursor and a separate IME anchor.
