# Presentation contracts

## Event surfaces

- Every visible normalized event MUST project through `ActivityRow`, `TranscriptBlock`, `ContentCard`, or `InputAccessory` before rendering.
- `TimelineModel` MUST own the sole transcript store and projector. Production render paths MUST NOT add provider/event-specific top-level surfaces.
- Surface replacement MUST remove the shadowed owner and insert the replacement at its original position. History prepend MUST NOT revive shadowed content or regress newer page state.
- Unknown compatible events MAY render a bounded fallback; unknown raw payloads MUST NOT cross the adapter boundary.

## Layout and cache

- Layout width is the resolved pane content width. Wrapping and clipping MUST operate on Unicode grapheme clusters and MUST NOT split combining sequences or emoji ZWJ clusters.
- Wrapping MUST preserve UAX #14 break opportunities, keep glued punctuation from starting rows, and fall back to grapheme splitting only for over-wide atoms.
- Structural changes may rebuild layout. Streaming and paced reveal MUST splice only the affected suffix; spinner frames MUST patch active ranges. Reading and Preview navigation MUST NOT flatten or rebuild transcript semantics.
- Markdown inline styling MUST consume parser events, not reparse flattened text. Tables, lists, quotes, code blocks, and provenance MUST share width-aware layout rules.

## Reveal and Preview

- Semantic transcript and Preview content MUST remain complete; reveal progress is presentation-only.
- Replay/history and copy operations MUST use complete semantic source. Plain-color mode may disable interpolation but MUST preserve pacing behavior.
- Preview selection, scroll, layout cache, and reveal progress are independent of transcript state. Stale async results MUST be rejected by target identity and revision.
- Tool Preview MUST use provider-neutral seeds. File mutation previews use event-supplied fragments; the frontend MUST NOT read files or compute missing diffs.

## Screen and copy

- Rendering layers point downward as Screen to Pane to Region to Component. Provider-specific rendering is forbidden.
- Detailed help MUST use one centered screen-level Markdown modal for both the help action and the built-in slash help command. Opening help MUST NOT append transcript content or replace Preview content, and the modal MUST show resolved key bindings without enumerating built-in or runtime command catalogs.
- Visual mouse selection copies only committed visible cells, remains inside its starting pane, preserves graphemes, and excludes separators. Reading copy uses complete owning-block source instead.
- Quick-link discovery from the latest completed assistant Markdown MUST remain pure and bounded. URI, absolute-path, and workspace-relative targets MUST be classified explicitly. Chinese/full-width opening wrappers are soft local-path boundaries: discovery retains longest-first hypotheses, validation selects at most one existing interpretation, and no interpretation is retained when an ambiguous group has no existing path. Selected targets are deduplicated before the first 36 receive presentation-only tags; overlapping rendered matches use the longest selected target, while semantic and copied source remains unchanged.
- Selection holds an immutable presented snapshot while background reduction continues; cancellation or release restores live rendering without losing dirty work.
- The terminal hardware cursor MUST remain hidden; the TUI uses a software cursor and a separate IME anchor.
