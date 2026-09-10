## Why

Reaching the bottom of Preview currently restores automatic following and pinned command information, which can abruptly replace the user's viewport. The user requests that wheel scrolling remain manual at the bottom.

## What Changes

- Keep Preview in manual review after wheel input, including at either boundary.
- Preserve the manual anchor on same-target updates; retain target-change reset behavior.
- Cover ordinary text and overflowing command information with focused regression tests.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `unified-preview-pane`: reaching the bottom no longer restores automatic following.

## Impact

Shared e-tui Preview scroll state, renderer regression tests, and the documented interaction invariant. Initial automatic presentation and Main scrolling remain unchanged.
