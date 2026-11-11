# Implementation design

## 1. Baseline and goals
The uncommitted character renderer is the baseline. The [math renderer](../../../../../crates/e-tui/src/render/math.rs) owns bounded parsing and grids, `RenderLine` carries source provenance, and transcript/Preview caches retain text lines. `TerminalOwner` owns terminal setup and atomic submission; `Presentation` captures text before submission. Extend the active change for the user-requested image experiment without replacing these paths.

## 2. Overall approach
Attach optional formula metadata to the first row of a supported display grid. Assistant transcript and Markdown Preview register fully visible image rectangles after painting their text. A draw-scoped collector transfers these requests to a terminal-owned renderer. After UI snapshot capture, and before Ratatui submission, render eligible Sixel overlays. Capability probing uses ratatui-image only during terminal initialization. Rasterization uses RaTeX with embedded KaTeX fonts; explicitly enable rust-embed's debug-embed feature so development builds do not read font files at runtime.

## 3. APIs and data model
`MathImage` contains formula source, grid dimensions and foreground color, not protocol bytes. `RenderLine.math_image` is optional. Preview owns row-indexed metadata separately from its existing text-layout cache. The terminal owns a bounded success/failure cache keyed by formula, geometry and colors. A thread-local collector exists only for the synchronous draw closure; no backend or session state is stored globally. Requests retain the underlying cells so later modal/selection painting invalidates overlapping requests.

## 4. Algorithms and rules
Keep character layout authoritative, including reveal, scrolling and copy units. Only fully visible, unwrapped grids qualify. Streaming/revealing assistant blocks and selection use characters. Preview requires all image rows to survive reveal and viewport selection. Raster dimensions and font scale are checked before allocation; reject unsupported or missing embedded glyphs before invoking the renderer, preventing system-font fallback. Use an LRU byte/entry bounded encoded Sixel cache, with bounded new rasterizations per frame. No asynchronous layout changes or idle polling are introduced. Exceptions or render errors preserve characters. Actual output failures remain terminal errors rather than successful image submissions.

## 5. Fixed decisions and discretion
Initial image transport is Sixel, positively negotiated within a 500 ms startup deadline; other terminals and tmux/screen fall back rather than receiving speculative escapes. Pin ratatui-image 12.0.0-rc.3: 11.1.0 leaves a detached stdin-reading thread on query timeout, while the selected version polls synchronously. Disable unrelated protocol probes. Font metrics are startup-only; font zoom requires restarting for recalibration. No environment/config override, subprocess renderer, external fonts, or graphics in inline math/user cards. Reuse grid dimensions to avoid late layout jumps; very small output or partially clipped formulas fall back. Cache sizes and helper organization are local implementation choices. Blocked: none.

## 6. Verification and documentation impact
Build the workspace; run existing renderer, layout, Preview, terminal and selection checks without changing tests. Inspect raster samples and, where available, terminal output visually. Run formatting and Clippy, reporting existing diagnostics separately. Update current presentation/architecture facts only for the delivered overlay boundary. Physical terminal compatibility is not established by compilation or text snapshots. Do not complete/archive the change.
