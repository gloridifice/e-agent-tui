<!-- doco:change mode=proposal-only -->
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
- Image rendering, terminal graphics negotiation, full TeX/package compatibility, automatic formula line breaking, and new test authoring are not included. The experimental image implementation is retained on a separate feature branch; the main branch remains character-only.

## Verification
- `cargo check --workspace --locked` and `cargo build -p e-tui --locked`: passed.
- Existing `render::tests`, `transcript_layout`, and `ui::region::preview` library tests: 52 passed; no tests added or changed.
- Manual renderer-output inspection at 40 and 5 columns covered all four delimiters, fractions, matrices, scripts, code protection, incomplete/unsupported formulas, macro rejection, clipping, and original copy units.
- `cargo fmt --all --check`, `git diff --check`, and `doco check terminal-math`: passed.
- Strict package Clippy is blocked by 33 diagnostics in existing code; none refer to the new math modules. Physical terminal/font compatibility remains unverified. The separately retained image experiment is not part of this branch's delivery.

## Result
Pending — implementation is delivered; lifecycle completion and archive were not requested.
