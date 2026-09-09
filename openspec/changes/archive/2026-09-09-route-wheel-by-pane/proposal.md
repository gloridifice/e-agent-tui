## Why

Wheel events discard pointer coordinates and always scroll the main pane, making Preview content inaccessible by mouse.

## What Changes

- Preserve wheel coordinates and route scrolling to the pane under the pointer, including History and narrow Preview-only presentation.
- Give Preview bounded manual scrolling including row zero without conflating it with automatic tail following.
- Update mouse help and add focused routing/rendering regressions.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `application-mouse-selection`: coordinate-directed wheel routing and selection cancellation.
- `unified-preview-pane`: independent bounded manual wheel scrolling.

## Impact

Shared e-tui terminal normalization, controller, Preview presentation state and renderer. No adapter-specific behavior or new keys. Preserve main-pane history paging, Preview caches, and automatic following until manual scrolling.
