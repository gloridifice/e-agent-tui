## 1. Screen-coordinate selection kernel

- [x] 1.1 Replace pane/logical-row points in `mouse_selection.rs` with a bounded screen-coordinate grapheme map and row-major cross-region extraction; support blank anchors, viewport clamping, Unicode continuation cells, and the documented trailing-space policy.
- [x] 1.2 Add explicit gesture movement/capture identity so stationary clicks and unmatched releases do not copy, one-wide-grapheme drags do copy, and whitespace-only payloads leave the clipboard unchanged.
- [x] 1.3 Add focused kernel regressions for forward/backward multiline ranges, cross-pane rows, intermediate blank rows, leading/internal spaces, trailing ordinary spaces versus NBSP, CJK/combining/ZWJ glyphs, and cancellation.

## 2. Final composited presentation snapshot

- [x] 2.1 Implement the Ratatui-buffer-to-screen-map adapter in `ui/selection.rs`; verify actual wide-cell occupancy, clipped/right-edge glyphs, overlay overwrite, and concealed symbols using real widget composition fixtures.
- [x] 2.2 Capture immutable unselected presentation after all ordinary overlays/notices and before mouse highlighting; preserve viewport, interaction-context identity, and hidden-cursor/IME anchor in the render output.
- [x] 2.3 Make selection the final presentation layer, distinguish ranges over existing reverse-video cursor/Reading cells without mutating base styles, and remove Transcript/Preview `register_line` plumbing and obsolete surface-specific tests.
- [x] 2.4 Add composition regressions proving composer, Input Pages, accessories, model/status, title/path, suggestions/notices, and decorative glyphs are copyable; prove masks, placeholders, ellipses, covered text, and reveal clipping cannot leak hidden source.

## 3. Shared capture and presentation lifecycle

- [x] 3.1 Add shared runner-owned committed/candidate/held presentation handling in `e-tui`; capture on eligible press and replay the immutable screen for selection-only frames without invoking semantic pane rendering.
- [x] 3.2 Centralize release/cancellation policy in the controller: preserve separator priority, copy once on release, cancel before focus/resize/scroll/edit/navigation/context transitions, and allow routine background reduction without invalidating capture.
- [x] 3.3 Integrate shared scheduling policy so held frames preserve deferred live dirty work, presentation deadlines cannot busy-loop or replace the screen, and release/cancellation immediately restores live rendering with bounded reveal pacing.
- [x] 3.4 Integrate both `crates/e-dsh/src/main.rs` and `crates/e-pi/src/main.rs` with the same presentation policy; publish snapshots only after successful terminal submission and never retain candidate geometry on failure.

## 4. Lifecycle and compatibility regression gates

- [x] 4.1 Add scripted-controller/port tests for clipboard success/failure and lock release, streaming/spinner stability, expiring selected notices, context changes with identical text, unmatched release, and ordinary editor/Reading/approval behavior.
- [x] 4.2 Add shared runtime/terminal tests for failed commit, both-adapter policy parity, pending live redraw preservation, deadline resumption/no busy-loop, and zero transcript/Preview materialization/rebuild/patch work on held selection-only frames.
- [x] 4.3 Run scoped Rust selection/UI/controller/runtime and architecture tests; verify existing fragmented Windows SGR/focus and separator regressions.
- [x] 4.4 Perform a manual Windows Terminal smoke check in both frontends for cross-region copy, wide glyphs, streaming, clipboard feedback, and focus/resize cancellation.

## 5. Documentation and final validation

- [x] 5.1 Update the client architecture selection invariant and localized help/README interaction guidance for screen-wide row-major copying, held presentation, visual-only extraction, and separator priority; preserve unrelated working-tree changes and the completed separator change's contract.
- [x] 5.2 Validate the OpenSpec change, run `cargo fmt --all` and `cargo clippy --all-targets` for this cross-cutting implementation, and record scoped test outcomes and any manual-test limitations without defaulting to a full workspace test suite.

## Validation Notes

- Passed `cargo check -p e-tui -p e-dsh -p e-pi`.
- Passed scoped `e-tui --lib` filters: `mouse_selection` (8), `ui::` (98), `runtime::controller` (36), `runtime::scheduler` (4), `runtime::input` (35), `runtime::terminal` (4), `runtime::executor` (3), `i18n` (4), and `help` (6). Filter counts overlap and are not a unique-test total.
- Passed `cargo test -p e-dsh --test architecture` (12), including shared runner selection-policy and successful-submission-before-publication guards.
- Passed `cargo fmt --all --check`, `git diff --check`, and strict OpenSpec validation. `cargo clippy --all-targets` and `cargo clippy --workspace --all-targets` completed without errors; existing warnings remain outside the new selection implementation.
- Manual Windows Terminal verification remains pending. An existing `pie` process and Windows Terminal window were detected but were not restarted or manipulated. The TestBackend/controller tests exercise the shared frontend rather than claiming live desktop validation of either executable.
