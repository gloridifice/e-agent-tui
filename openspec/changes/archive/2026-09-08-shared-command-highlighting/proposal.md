## Why

The approved terminal prototype makes executable names, flags, and shell operators easier to distinguish. Preview and History should share that presentation without duplicating command parsing.

## What Changes

- Add a provider-neutral, presentation-only command highlighter shared by Preview and History.
- Use Blush-equivalent executable names, Mist-equivalent arguments and italic flags, and Bark-equivalent chain/redirection operators; quoted contents stay ordinary argument text.
- Preserve source, existing wrapping/clipping, Preview chrome/cache, History metrics, and undecorated exports. Add no backgrounds or borders.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `structured-tool-preview`: command-token presentation in structured and standalone command Preview.
- `session-execution-history`: apply shared command presentation to command summaries, retaining history semantic theme ownership.

## Impact

`e-tui::ui::component` owns the reusable lexer/styled-text interface; Preview and History regions supply existing semantic foregrounds. No theme schema, adapter, wire, persistence, or dependency change. Focused tests cover token boundaries, quoted/escaped operators, redirection targets, multiline input, and surface integration rather than theme palettes.
