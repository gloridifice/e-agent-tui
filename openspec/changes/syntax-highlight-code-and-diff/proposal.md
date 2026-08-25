## Why

Code fences and mutation previews currently render mostly uniform text, which makes model-produced code and edits harder to scan. The client already owns semantic Markdown, diff, Preview, reveal, and cache layers, so syntax color should be added through those layers without giving up theme coherence or bounded rendering work.

## What Changes

- Add syntax highlighting for fenced code blocks in transcript Markdown and for code bodies shown in unified or structured diff previews.
- Add `semantics.markdown_weak`, with the same roles as `semantics.markdown`, for Markdown rendered inside Preview; bundled themes map it only to Bark, Umber, and Night equivalents while retaining bold, italic, and underline modifiers.
- Keep transcript code fences and diff syntax tokens on `semantics.markdown`; keep Preview Markdown code fences on `semantics.markdown_weak`.
- Keep diff row structure (added/removed backgrounds, accents, gutters, and separators) under `semantics.diff`, layered with syntax foregrounds from `semantics.markdown`.
- Preserve the path or language hint needed to select a syntax, and fall back to existing code text styling for unknown languages, malformed input, highlighting failure, or bounded-size limits.
- Cache styled syntax layouts so ordinary redraws, Preview reveal/fade, scrolling, and Reading navigation do not rerun syntax parsing.
- **BREAKING**: the fixed theme schema gains the required `semantics.markdown_weak` group; custom theme files must add the new group.

## Capabilities

### New Capabilities
- `syntax-highlighted-code`: Syntax selection, semantic token coloring, diff composition, graceful fallback, and bounded cached rendering for code fences and diff bodies.

### Modified Capabilities
- `unified-preview-pane`: Preview Markdown uses the new weak Markdown semantic group while diff syntax deliberately uses the normal Markdown group.
- `structured-tool-preview`: Injected-context Markdown changes from a single forced muted foreground to the weak Markdown semantic hierarchy, and event-authored mutation previews gain syntax-colored code bodies without client-authored diffs.

## Impact

- Affects `crates/e-tui` Markdown rendering, theme schema, Preview semantic values/layout cache, diff components, reveal-compatible styled lines, and UI regression tests.
- Affects both bundled theme TOML files and invalidates older custom theme files until they add `semantics.markdown_weak`.
- Adds `tui-syntax-highlight` and `syntect` to `crates/e-tui`, using embedded syntax assets with no render-time filesystem I/O.
- May adjust provider-neutral diff Preview data to retain an optional path while leaving the bridge protocol unchanged.
- Requires updates to `docs/client.md`; README changes are not expected.
