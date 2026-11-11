# Execution tasks

- [x] 1.1 Add bounded, embedded-font rasterization and terminal-owned Sixel composition
  - Design: [implementation](implement.md)
  - Acceptance: Positive capability detection, bounded cache, no formula-triggered I/O, and character fallback on failure; no graphics bytes in captured text.
- [x] 1.2 Carry display-formula metadata through assistant and Preview viewports
  - Dependencies: 1.1
  - Acceptance: Fully visible supported formulas prefer images; clipping, reveal, selection, modal overlap and unsupported surfaces retain safe character output; source copy stays unchanged.
- [x] 2.1 Validate integration and synchronize current contracts
  - Dependencies: 1.1, 1.2
  - Acceptance: Scoped existing checks and manual raster inspection recorded; current documentation reflects delivered behavior and explicitly reports unverified terminal compatibility.

## Verification
- Workspace `cargo check --workspace --locked`, `cargo build -p e-tui`, `cargo fmt --all --check`, and `git diff --check` passed. Workspace/all-target Clippy completed with existing warnings and no diagnostics in the new image path.
- Existing e-tui library filters `render::tests`, `transcript_layout`, `ui::region::preview`, `runtime::terminal`, and `ui::selection`: 61 passed. No tests were added or edited.
- Existing e-dsh architecture checks: 13 passed, including acyclic production dependencies and rendering layering; three runner-source assertions failed because they inspect the old Pi main entry point. The test and both inspected adapter main files are byte-identical to HEAD (`git diff --exit-code HEAD --` those files passed). They were not changed to accommodate this delivery.
- An out-of-tree renderer sample produced and visually inspected quadratic-fraction, summation and matrix PNGs; blackboard/script and Euler samples also rasterized and Sixel-encoded. A CJK sample retained the character fallback. The sample used the production raster implementation and final dependency artifacts. `cargo tree -p e-tui -e features -i rust-embed` confirmed debug-embed activation.
- Embedded font OFL/copyright notices were retained in assets and added to binary release staging. The copyright statements were checked against all 20 embedded TTF name tables.
- Current architecture, presentation, and archive-content documentation were synchronized. Real-terminal Sixel negotiation, scrolling/erasure, font zoom, and physical selection interactions remain unverified; no terminal-family compatibility claim or release was made. Font metrics require restart after zoom. Other graphics protocols, inline/user-card images, and multiplexer passthrough remain deferred.
