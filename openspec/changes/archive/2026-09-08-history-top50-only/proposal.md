## Why

History should offer only the session-wide Top 50 elapsed ranking, not a turn browser or view toggle.

## What Changes

- Open the full-session ranking directly; remove grouped turn browsing, timeline charts, and incremental turn-page requests.
- Keep one ranked list, its operation legend, independent scrolling, diagnostics, and fixed navigation footer in the message pane.
- Remove the Tab toggle and its hints. Recognize the retired `history.toggle_view` override as ignored compatibility input so existing mappings still load.
- Keep chronological/copy-10 exports, ranking eligibility, trace recording, and split Preview unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `session-execution-history`: ranking-only presentation and navigation replace turn/timeline and toggle requirements.

## Impact

Shared `e-tui` history state, controller, rendering, key mappings, help, and documentation. Reuse the existing adapter full-session Top 50 query without changing persistence or export formats. Checked `full-screen-pages` and `configurable-key-mapping`: navigation ownership and strict rejection of unknown mappings remain; the retired toggle is explicitly recognized, not an unknown key.
