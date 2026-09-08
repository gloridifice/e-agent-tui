## Context

`e-tui` currently parses Markdown itself, materializes styled `RenderLine` values with stable source provenance, and renders Preview through a separate semantic cache and row-reveal sidecar. Fenced code uses one `markdown.code_text` style. Raw unified diffs and event-authored old/new fragments use diff-level colors but do not color the code inside each row.

The in-progress working tree also introduces a dedicated `semantics.diff` group and a richer diff row component. This change must complete against that direction rather than restore the removed flat `diff::added`/`diff::removed` helpers.

The design must retain these constraints:

- `e-tui` remains kernel-neutral and performs no syntax or theme filesystem I/O.
- Transcript Markdown keeps atomic code provenance, collapse behavior, cached width-aware layout, and presentation-only reveal.
- Preview syntax work must not run on every frame or invalidate transcript layout.
- The client may classify event-authored diff rows but must not read files or compute a diff.
- Themes are strict fixed-semantic TOML documents; adding a required group is a schema migration.

## Goals / Non-Goals

**Goals:**

- Highlight fenced code and event-authored diff bodies using embedded syntax definitions.
- Choose token colors from normal or weak Markdown semantics according to content context.
- Preserve diff structural styling independently from syntax token styling.
- Preserve complete source, copy provenance, row reveal/fade, responsive layout, and bounded render work.
- Degrade safely when language detection or highlighting is unavailable.

**Non-Goals:**

- Reading target files, deriving missing context, or calculating edit differences.
- Adding user-installed Syntect grammars or `.tmTheme` files.
- Adding a configurable syntax-theme selector in this change.
- Highlighting arbitrary terminal output, generic JSON, command text, or plain-text Preview values.
- Changing Markdown colors already exposed by `semantics.markdown`.

## Decisions

### 1. Use `tui-syntax-highlight` as a span adapter, not as a layout component

`e-tui` will add `tui-syntax-highlight` 0.2 and `syntect` 5 with default features disabled, embedded default syntaxes enabled, and the pure-Rust `regex-fancy` engine selected. The crate will produce Ratatui spans only. Existing renderers continue to own headers, gutters, line numbers, collapse windows, clipping, backgrounds, provenance, wrapping, and reveal.

This avoids a second widget/layout model and avoids the native Oniguruma build in the Windows-first source installation path. `regex-onig` is faster, but bounded cached highlighting makes build portability the stronger initial trade-off. A release benchmark will verify that decision.

The embedded `SyntaxSet` will be initialized once through `OnceLock`; no syntax asset is read from disk. Initialization and highlighting receive Tracy zones so cold and steady-state costs remain observable.

### 2. Add a normal/weak Markdown style context

`Theme` will gain a required `markdown_weak: MarkdownTheme`; `MarkdownRef` and `MarkdownTheme` keep exactly the same role set for normal and weak groups. `RenderOptions` (or an equivalent internal context value) will select `Normal` or `Weak`, defaulting to `Normal` so current transcript and example call sites retain behavior.

Normal Markdown rendering reads `semantics.markdown`. Markdown rendered as Ready Preview content, including reasoning and injected context, reads `semantics.markdown_weak`. The current post-render operation that forces every foreground to one Bark-equivalent color will be removed. Diff is a content-type exception: its syntax tokens use normal Markdown even though the diff is displayed in Preview.

Both bundled themes will define every weak role using only `bark`, `umber`, and `night`, while retaining selected bold, italic, and underline modifiers. `deepseek-e` will add a `night` palette alias without changing existing `midnight` references. The intended weak role mapping is:

- Bark: primary text, headings 1-5, emphasis/strong, inline code, links/images, code text/background foreground, table headers, checked tasks, Mermaid nodes/labels/title.
- Umber: heading 6, strikethrough, quote/rule, code metadata, table border, list marker, unchecked tasks, Mermaid border/edge.
- Night: inline-code and code-block backgrounds.

The group is required rather than silently synthesized. Existing custom themes that omit it become illegal and fall back through the existing theme-resolution behavior until their authors migrate them.

### 3. Derive Syntect themes from Markdown semantic roles

The syntax layer will construct/cache a Syntect theme from the selected `MarkdownTheme`; it will not load Syntect's bundled color themes. Scope families map as follows:

| Syntax scope family | Markdown role |
| --- | --- |
| default text, variable, punctuation, operator | `code_text` |
| comment, documentation | `code_meta` |
| keyword, control, storage, modifier | `heading1` |
| type, class, trait, interface, enum | `heading2` |
| function, method, macro | `link_text` |
| string, character, regular expression | `emphasis` |
| number, boolean, null, constant | `inline_code` |
| attribute, annotation, decorator | `heading4` |
| escape, interpolation | `link_url` |
| invalid, deprecated | `strikethrough` |

Only foreground, bold, italic, and underline are transferred to token styles. Role backgrounds are ignored at token level. Code-block background remains `code_background`; diff-row background remains `semantics.diff.added` or `semantics.diff.removed`.

This keeps syntax colors coherent with existing and custom project themes and avoids another public syntax-token schema. A fixed external Syntect theme was rejected because it would conflict with Ferra, DeepSeek E, weak Preview Markdown, and theme reload.

### 4. Resolve syntax from semantic hints with a safe fallback

Fenced code resolves a normalized first language token with `SyntaxSet::find_syntax_by_token`; a small alias table covers common fence names not recognized directly. Diff code resolves from the explicit provider-neutral path first, then from an event-provided unified-diff file header when available. Unknown/empty hints use plain text.

Highlighting failure, malformed diff structure, excessive input, or an excessive individual line returns existing semantic code-text spans rather than a Preview error. Limits will be explicit and at least as strict as the existing bounded Preview path (256 KiB and 2,000 rows), with an additional long-line guard.

### 5. Preserve path in diff Preview semantics and classify without computing

`PreviewContent::Diff` will retain `source` plus an optional `path`; `ToolReference::Diff` already has that path, so this is an `e-tui` semantic-model correction rather than a bridge protocol change.

A small unified-diff classifier will recognize file headers, hunk headers, added, removed, context, and metadata rows and track event-authored old/new line coordinates. For each hunk it will highlight two logical streams:

- old stream: context plus removed rows;
- new stream: context plus added rows.

Removed rows use old-stream spans; added and displayed context rows use new-stream spans. This preserves useful multiline parser state without interleaving mutually exclusive old and new code. Structured `MutationHunk.old` and `.new` values are highlighted independently by the same rule. The classifier never creates text rows, context, or edit operations.

The diff component will accept styled body spans rather than a plain body string. It will prepend its project-owned gutter and line number, apply the diff background to every syntax span, and truncate styled spans grapheme-safely while preserving foregrounds and modifiers.

### 6. Cache highlighted layouts at the owning presentation layer

Transcript code highlighting occurs only when `MarkdownLayoutRegistry` rematerializes the affected Markdown block. Stable code blocks therefore reuse existing cached `RenderLine` values; streaming changes rematerialize only the affected transcript suffix under existing cache discipline.

Preview will gain a width/style-aware materialized-line cache separate from its semantic `PreviewCache`. Its production identity includes Preview key/revision, content width, and a signature of the selected semantic styles. Direct Ready renderer fixtures may use a bounded content signature. Preview reveal/fade, scrolling, mouse selection, and unchanged redraws reuse those styled rows. Theme reload and width change invalidate styled layout only and reconcile the existing semantic reveal frontier.

Syntax results never enter copy payloads or semantic cache keys. Generated fill padding remains presentation-only.

### 7. Preserve reveal, copy, and atomic code semantics

Fenced code continues to emit the same atomic unit/raw-line ownership and collapse header/tail rows. Syntax spans only replace the current single `code_text` body span. Copy and Reading View continue to use complete original fenced source.

Preview row reveal continues to wrap before pacing and fades each syntax span toward its own semantic foreground while preserving code/diff backgrounds and modifiers. No syntax state is stored in reveal tracks.

## Risks / Trade-offs

- **[Pure-Rust regex highlighting is slow in debug builds]** → Cache all styled layouts, bound content and line length, exercise release benchmarks, and switch to `regex-onig` only if measured release performance cannot meet the redline.
- **[Cold syntax-set initialization can exceed one frame budget]** → Measure an explicit cold zone; keep initialization one-time and, if necessary, warm it before the interactive loop rather than initializing repeatedly in render code.
- **[Strict theme schema breaks custom themes]** → Document the required group, update both bundled files together, and rely on existing invalid-theme fallback rather than partially accepting malformed semantics.
- **[Syntect scopes vary across grammars]** → Use broad ordered scope selectors and test representative Rust, JavaScript/TypeScript, Python, shell, and unknown-language inputs.
- **[Unified diffs can contain multiple files or malformed headers]** → Track file hints per event-authored section and fall back row-by-row without dropping or inventing source.
- **[Styled truncation can split Unicode or lose backgrounds]** → Reuse the project's grapheme/display-width conventions and assert complete diff backgrounds with TestBackend tests.
- **[Streaming code may repeatedly rematerialize a growing fence]** → Keep work limited to the existing dirty suffix, enforce highlighting limits, record highlight counts in logical tests, and benchmark continuous streaming.

## Migration Plan

1. Complete the in-progress `semantics.diff`/diff-component migration so the baseline builds.
2. Add `markdown_weak` to the theme schema and both embedded theme files; add the DeepSeek E `night` alias.
3. Add syntax dependencies and the kernel-neutral syntax adapter with fallback and limits.
4. Integrate normal/weak Markdown selection and fenced-code spans without changing provenance.
5. Preserve diff paths, classify event-authored rows, and compose syntax spans with the diff component.
6. Add Preview styled-layout caching and hook width/theme invalidation into existing paths.
7. Add renderer/UI/cache regressions, scoped tests, release timing checks, and update `docs/client.md`.

Rollback removes the syntax adapter and returns code bodies to their existing semantic text style. Theme files containing `markdown_weak` are not backward-compatible with an older strict parser, so rollback also requires restoring the prior bundled theme files.

## Open Questions

- The initial scope excludes `PreviewContent::Lines` file previews. It can reuse the same path-based highlighter later, but this change is limited to Markdown code fences and diff bodies.
