## Why

Model and effort changes need immediate visual confirmation without adding status text. The approved prototype flashes the new value and smoothly fades back to its normal foreground.

## What Changes

- Add independent model and effort status flashes in the shared TUI, lasting 0.6 seconds.
- Model uses Mist; effort uses Ember for max, Honey for xhigh, Blush for high, and Mist otherwise, resolved by effective effort id rather than display label.
- Trigger on confirmed changes, not initial hydration or repeated catalog refreshes; retain existing labels and modifiers.
- Schedule bounded presentation-only frames and stop when both flashes finish.

## Capabilities

### New Capabilities

- `status-selection-feedback`: confirmed selection flashes and their lifecycle.

### Modified Capabilities

- `reasoning-effort`: permit the transient foreground over the normal dim effort style.

## Impact

Shared `e-tui` catalog resolution, presentation sidecars, model catalog reduction, and status rendering; no adapter protocol, configuration format, or key changes. Checked `terminal-render-performance` and `turn-model-prefix`: preserve idle scheduling, cache isolation, and temporary-model italics.
