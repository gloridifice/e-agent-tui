## 1. Implementation
- [x] 1.1 Implement bounded candidate extraction, workspace-relative normalization, stale-safe target state, and adapter-owned containment validation; test mixed syntax, confidence, missing paths, traversal, and symlinks.
- [x] 1.2 Integrate settlement-triggered validation and presentation-only tags with Markdown layout; test ordering/capacity, latest-message replacement, source preservation, and wrapping.
- [x] 1.3 Add configurable copy-entry action and tag-key handling with existing clipboard effects; update help/reference and test cancellation, protected contexts, remapping, and stale results.
- [x] 1.4 Run focused cross-boundary checks, workspace formatting and Clippy, update the architecture boundary, and merge/archive the validated specification.

Validation: quick-link, controller, terminal-input, Markdown/layout, clipboard executor, localization, both adapter filesystem tests (including Windows junction escape), and architecture checks passed. Workspace binaries build and formatting pass. Workspace all-target Clippy completes with existing warnings; an additional `-D warnings` run is not clean. Physical terminal and live-provider acceptance remain for the user's manual verification.
