## 1. Implementation

- [x] 1.1 Preserve wheel coordinates and route by visible pane; verify split, History, single-pane, separator and resize cases with focused controller/input tests.
- [x] 1.2 Implement bounded Preview manual scrolling and tail restoration; verify row-zero, boundaries, target reset, and renderer/cache behavior with focused tests.
- [x] 1.3 Update mouse help and architecture text and run focused checks before CLI archive.

Verification: `cargo test -p e-tui --lib wheel`, `preview`, `selection`, `runtime::input`, and `long_tool_output_pins_information_and_shows_the_latest_tail` passed. Changed Rust files were formatted; `git diff --check` passed. No full-workspace test or live-terminal session was run.
