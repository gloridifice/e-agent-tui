## Why

Keyboard shortcuts are hardcoded across the shared frontend, preventing platform-appropriate customization and reliable help. Users need scoped TOML overrides, disabled bindings, and new model, effort, settings, and Reading shortcuts without losing existing editor and modal behavior.

## What Changes

- Add root `default_key_mapping.toml`, embedded by e-tui, and per-adapter `<config_path>/key_mapping.toml` overrides with string/array bindings, `nop`, exact modifiers, and platform-aware `osmain`.
- Route all existing keyboard interactions through semantic actions and explicit active scopes, with validation and atomic reload.
- **BREAKING**: default Reading entry becomes osmain-R (history search becomes unbound); osmain-L/E/comma open model/effort/settings. Reading Esc/q always exits, and Backspace returns from Items to Blocks.
- **BREAKING**: approvals reject only explicit deny bindings, and history search accepts literal q. Old customized-away shortcuts do not remain active.
- Generate help and UI key hints from effective mappings; preserve bracketed paste, mouse, Unicode editing, draft, queue, and provider boundaries.

## Capabilities

### New Capabilities
- `configurable-key-mapping`: declarative bindings, validation, scoped resolution, loading/reload, safe dispatch, and truthful hints.

### Modified Capabilities
- `semantic-reading-view`: configurable entry/navigation/copy, unambiguous whole-view exit and Item return.
- `input-page`: configurable browse/edit navigation and protected global page shortcuts.

## Impact

Shared e-tui input, pages, runtime routing, config runtime values, localized help/hints; e-dsh and e-pi configuration loading; terminal regression gates; README and client architecture. No wire changes or new dependencies are expected.
