## Why

Tool previews currently expose disconnected `Path`, `Command`, `PlainText`, and synthesized `Diff` values, so a completed tool often replaces its useful call context with raw output. A structured tool-preview contract is needed now to preserve tool identity, primary input, metrics, bounded output, and provider-authored mutation data in one consistent Preview presentation.

## What Changes

- Introduce a structured tool Preview with an Umber tool-name header, a typed primary section, and an optional secondary section separated by blank rows.
- Give `read`, `view`, `create`, filesystem search, `command`, `pwsh`, and `bash` dedicated primary presentations; render unsupported tool arguments as bounded pretty JSON.
- Keep command identity distinct in Preview (`command`, `powershell`, and `bash`), render `$` in Coral, command text in Mist, metrics in Bark, and terminal output through a safe two-tone ANSI mapper.
- Preserve ANSI bold and italic modifiers while mapping explicitly colored output to Bark and uncolored output to Umber; never pass terminal escape sequences through to the terminal.
- Render `edit`, `replace`, and `insert` as diff previews from event-provided mutation data without reading files or running a client-side diff algorithm.
- Render prompt-injection/context content as muted Markdown while leaving the existing reasoning/thinking presentation unchanged.
- Preserve bounded payload, Preview cache, Reading View, history replay, and race-safe selection behavior.

## Capabilities

### New Capabilities
- `structured-tool-preview`: Defines the common tool Preview layout, per-tool content, ANSI mapping, diff behavior, generic JSON fallback, and muted prompt-injection Preview.

### Modified Capabilities
- `dsh-event-projection`: Requires DSH tool arguments and result presentation metadata to be normalized into typed, bounded preview facts at the adapter boundary rather than interpreted by `e-tui` renderers.

## Impact

- `crates/e-dsh/src/protocol/host_event/tool.rs` and `crates/e-dsh/src/bridge/adapter.rs`: typed extraction and normalization of tool input, result output, and provider-supplied mutation metadata.
- `crates/e-tui/src/agent/tool.rs`, `preview.rs`, projection/application state, and `ui/region/preview.rs`: structured preview ownership, call/result correlation, rendering, ANSI parsing, and tests.
- `docs/client.md` and `docs/design.md`: Preview format, color semantics, DSH mutation-data limitations, and fallback rules.
- No new filesystem reads are introduced. A bridge wire-version change is not expected because the required call arguments, result output, and result `meta` already travel inside existing host events; malformed or trimmed optional metadata degrades to the bounded generic/path fallback.
