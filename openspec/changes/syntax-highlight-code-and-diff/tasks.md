## 1. Baseline and Theme Schema

- [x] 1.1 Complete the in-progress `semantics.diff` and styled diff-row migration so `cargo check -p e-tui` passes before syntax work is layered on it.
- [x] 1.2 Add `tui-syntax-highlight` and `syntect` with embedded default syntaxes and the pure-Rust regex engine to `crates/e-tui/Cargo.toml`, then update the workspace lockfile.
- [x] 1.3 Add required `markdown_weak` theme schema/resolved fields with the same role set as `markdown`, and add parser tests for complete, missing, and illegal weak groups.
- [x] 1.4 Add Bark/Umber/Night-only `semantics.markdown_weak` mappings to both bundled themes, including the DeepSeek E `night` alias, without changing existing normal Markdown mappings.

## 2. Syntax Highlight Adapter

- [x] 2.1 Add a kernel-neutral syntax module that initializes the embedded `SyntaxSet` once, normalizes fence/path hints, resolves common aliases, and performs no filesystem I/O.
- [x] 2.2 Implement the documented Syntect-scope-to-`MarkdownTheme` mapping, transferring foreground/bold/italic/underline while suppressing token backgrounds.
- [x] 2.3 Add bounded byte/row/long-line guards, local plain-code fallback, and Tracy zones for cold initialization and highlighting work.
- [x] 2.4 Add focused syntax-module tests for normal and weak Rust styles, representative aliases, unknown syntax, limits, and failure fallback.

## 3. Markdown Code Blocks

- [x] 3.1 Add a normal/weak Markdown render context that defaults to normal and routes every Markdown role, including top-level list markers and Mermaid fallback styles, through the selected group.
- [x] 3.2 Replace fenced code body `code_text` spans with syntax-highlighted spans while preserving headers, fill backgrounds, raw-line ownership, atomic units, collapse windows, and complete source.
- [x] 3.3 Render Preview Markdown, reasoning, and injected-context Markdown with `markdown_weak`, removing the post-render forced-Bark foreground rewrite.
- [x] 3.4 Add renderer and TestBackend regressions proving normal versus weak code colors, ignored token backgrounds, retained code fill, collapse geometry, modifiers, and complete atomic copy provenance.

## 4. Diff Syntax Composition

- [x] 4.1 Change provider-neutral diff Preview semantics to retain optional path plus source, and update tool-reference conversion, cache equality, revision fixtures, and all affected constructors without changing the bridge protocol.
- [x] 4.2 Implement bounded unified-diff classification for file/hunk metadata, event-authored old/new coordinates, row kinds, and per-file syntax hints without computing or inventing diff content.
- [x] 4.3 Highlight old and new logical streams independently for unified sections and structured mutation hunks, with malformed and unknown-language fallback that retains every event-authored row.
- [x] 4.4 Update the diff component to accept styled body spans, compose normal-Markdown syntax foregrounds with `semantics.diff` structure, fill added/removed backgrounds, and truncate across spans grapheme-safely.
- [x] 4.5 Integrate classified raw diffs and structured hunks into Preview and add TestBackend regressions for syntax foregrounds, line numbers, full-row backgrounds, multi-file sections, multiline state, Unicode truncation, and malformed input.

## 5. Preview Cache and Reveal Integration

- [x] 5.1 Add a width- and theme-style-aware Preview materialized-line cache separate from semantic `PreviewCache`, keyed by selected target key/revision with a bounded direct-Ready fixture fallback.
- [x] 5.2 Reuse styled syntax rows across ordinary redraw, scroll, selection, reveal, and fade; invalidate only styled layout on width/theme changes and preserve the semantic reveal frontier.
- [x] 5.3 Add logical work-count tests proving repeated Preview frames do not rerun syntax highlighting or invalidate transcript cache, while revision, width, and theme changes rematerialize only the affected Preview layout.
- [x] 5.4 Verify syntax spans participate in existing transcript/Preview fade with their own foregrounds while preserving code/diff backgrounds and modifiers.

## 6. Validation and Documentation

- [x] 6.1 Run scoped `e-tui` theme, syntax, Markdown-renderer, Preview, diff, reveal, and UI tests; fix regressions without running unrelated bridge tests.
- [x] 6.2 Run release timing scenarios for cold syntax initialization, repeated highlighted Preview frames, streaming code, and large bounded diffs; confirm cached steady-state work and the documented P95 frame redline.
- [x] 6.3 Run `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets` for the large cross-cutting Rust change.
- [x] 6.4 Update `docs/client.md` and `AGENTS.md` in English with weak Markdown semantics, syntax/diff ownership, fallback limits, cache rules, theme migration, and validation commands; keep README unchanged unless user-facing basics materially change.
