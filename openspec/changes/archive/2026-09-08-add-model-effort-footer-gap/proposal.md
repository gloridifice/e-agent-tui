## Why

Model and effort options currently touch the key-hint footer. Separate them with one blank row.

## What Changes

- Reserve one blank row above the `/model` and `/effort` footer, including when lists scroll.
- Include the gap in preferred page height without changing other pages.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `input-page`: blank separation before model and effort key hints.

## Impact

Scoped to `crates/e-tui/src/ui/pages/` layout and regression tests. Preserve existing terminal height caps and focus scrolling.
