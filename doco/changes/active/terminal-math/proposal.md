<!-- doco:lifecycle v=1 created-at=2026-11-11T08:51:24Z completed-at=- archived-at=- -->
# Terminal math

## Purpose
Make mathematical Markdown readable in the shared terminal frontend without requiring graphics protocols, external programs, or changing copied source.

## Scope and acceptance
- Recognize `$...$` and `\(...\)` inline math, and `$$...$$` and `\[...\]` display math in top-level paragraphs through the existing Markdown parser. The backslash forms follow the OpenAI GPT-5 prompting guide and standard LaTeX conventions. Code remains literal. Nested display layout is outside this delivery; nested display events retain their LaTeX representation. Delimiter recognition must preserve original byte offsets and protect code, HTML, and link targets.
- Render supported formulas as Unicode using `term-maths` layout. Use the fallible `pulldown-latex` event parser rather than the permissive shorthand parser, adapting supported events to layout nodes. Unsupported syntax and layouts preserve the formula source rather than silently dropping content.
- Inline formulas must stay on one logical row; formulas requiring multiple rows retain LaTeX. Paragraphs containing display formulas own atomic source-copy rows; rendered formula grids clip horizontally to the resolved pane width instead of wrapping their geometry.
- Keep input, stored Markdown, history, whole-block copy, and clipboard effects unchanged. Reuse shared Markdown surfaces and their existing caches; do not add backend-specific rendering or persistent state.
- Bound source size, parser events, nesting, and output dimensions. Reject user-defined macros before parsing; perform no subprocess, filesystem, or network operations while rendering. Incomplete streaming formulas retain source until they can be rendered.
- Update the current presentation contract for delivered behavior. Do not change READMEs, keys, configuration, or tests. Validate using scoped existing renderer/layout tests, package compilation, and formatting checks.
- The follow-up adds image-first display formulas in assistant Markdown and Markdown Preview, initially using negotiated Sixel support. Inline math and user cards remain character-based. Full TeX/package compatibility, other image protocols, automatic formula line breaking, and new test authoring are not included.
- Images must be presentation-only overlays with bounded raster/encoding caches, embedded fonts, and no formula-triggered external execution or file access. Preserve character-grid geometry and source copy. Unsupported terminals, image failures, partial visibility, clipped grids, active reveal/selection, and obscured regions retain character output. Capability probing belongs to terminal setup, before event readers start. Graphics bytes must never enter Markdown, history, or selectable snapshots.

## Verification
- `cargo check --workspace --locked` and `cargo build -p e-tui --locked`: passed.
- Existing renderer, transcript layout, Preview, terminal lifecycle, and selection library tests: 61 passed; no tests added or changed.
- Manual renderer-output inspection at 40 and 5 columns covered all four delimiters, fractions, matrices, scripts, code protection, incomplete/unsupported formulas, macro rejection, clipping, and original copy units.
- `cargo fmt --all --check`, `git diff --check`, and `doco check terminal-math`: passed.
- Workspace/all-target Clippy completed with existing warnings. Architecture checks passed 13 of 16 checks; three unchanged runner-source assertions still reference the old Pi main entry point. No production dependency cycle was reported.
- Quadratic, summation, matrix, blackboard/script, and Euler samples rasterized and Sixel-encoded; representative PNGs were visually inspected. CJK glyph fallback remained textual. Debug builds embed fonts, and font notices are included in source packaging and binary release staging.
- Actual Sixel terminal negotiation, scrolling/erasure, and physical selection remain unverified. Other graphics protocols, inline/user-card images, and multiplexer passthrough remain deferred.

## Result
Pending — implementation is delivered; lifecycle completion and archive were not requested.
