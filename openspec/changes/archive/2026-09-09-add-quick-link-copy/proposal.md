## Why
Issue #24 requests fast copying of links and paths from the latest completed Markdown answer without mouse selection or requiring Markdown link syntax.

## What Changes
- Extract URI, Windows/POSIX absolute-path, and confidence-ranked workspace-relative candidates after assistant message completion.
- Validate relative candidates outside UI locks through adapter filesystem ports, rejecting lexical and symlink escapes.
- Label up to 36 distinct targets in output order using Umber `~1` through `~z`; keep source/copy provenance unchanged.
- Add configurable `global.copy_link` (Ctrl+Y) to enter one-key tag selection, reusing clipboard effects.

## Capabilities
### New Capabilities
- `quick-link-copy`
### Modified Capabilities
None.

## Impact
Shared candidate/state values, Markdown presentation and cache identity, controller/effect ports, both adapters, keyboard help and tests. The key mapping still parses single chords: the tag is input to a frontend selection mode, not a configurable macro.
