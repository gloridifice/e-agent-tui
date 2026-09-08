## Why

Frequently used models need a direct selection path inside `/model` without repeating provider and row navigation.

## What Changes

- Toggle a mark on the focused model with Shift plus an available ASCII letter; select it and close the page with that letter.
- Reserve letters used by effective page/global bindings, including their shifted chords.
- Persist one letter per provider/model route in shared frontend config; reassignment moves the letter and replaces the route's old mark.
- Render a Bark-equivalent ` [a]` suffix and document the interaction in localized help.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `openspec/specs/input-page/spec.md`: model letter marking, selection, persistence, and display.

## Impact

Frontend config, key resolution, model page, rendering, and help only; existing ModelSet and config-save effects remain authoritative. Read the input-page and declarative-config-overlay main specs and the agreed configurable-key-mapping delta. No adapter, transport, or general binding semantics change.
