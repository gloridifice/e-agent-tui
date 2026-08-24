## 1. Mouse event normalization

- [x] 1.1 Define frontend-neutral pointer gesture types for primary press, drag, release, wheel, and focus-loss cancellation, preserving terminal cell coordinates.
- [x] 1.2 Extend `route_terminal_event` and controller tests to route primary-button gestures separately from existing wheel scrolling and to ignore unsupported mouse input.
- [x] 1.3 Extend the Windows VT parser to emit normalized primary press, drag, and release events from complete and fragmented SGR reports while preserving its current wheel and paste behavior.
- [x] 1.4 Enable and restore terminal focus reporting symmetrically; parse raw-VT `CSI I`/`CSI O` focus reports on Windows, route focus loss through the pointer lifecycle, and add terminal/parser regression coverage.
- [x] 1.5 Convert Windows SGR mouse row/column values from one-based protocol coordinates to zero-based Crossterm cells and cover fragmented reports with regression tests.

## 2. Frontend selection model

- [x] 2.1 Add an `e-tui` leaf selection module with interaction-owned drag/selected state, normalized points and ranges, surface identity, epoch validation, and selection clearing rules.
- [x] 2.2 Implement grapheme-aware terminal-cell boundary maps and visual-row extraction helpers using existing Unicode segmentation and display-width conventions.
- [x] 2.3 Add `SelectionFrame` render sidecars containing only last-frame visible selectable Transcript and Preview rows, with hit testing and same-surface edge clamping.
- [x] 2.4 Integrate selection state into `InteractionModel` and expose committed-frame sidecars as runner-owned render artifacts without introducing terminal or clipboard dependencies into `e-tui`.
- [x] 2.5 Add focused model tests for forward/backward ranges, empty selections, generated-fill trimming, blank rows, CJK, emoji, and combining grapheme boundaries.

## 3. Render integration

- [x] 3.1 Collect Transcript selectable-row metadata inside existing visible wrapped-row materialization, excluding help-only, input, accessory, status, title, and generated padding cells.
- [x] 3.2 Collect Preview selectable-row metadata after its wrap, scroll, and centering decisions, and keep Transcript/Preview hit regions independent in split and full-screen layouts.
- [x] 3.3 Publish selection geometry only after a successful terminal frame transaction; retain the previous committed geometry when drawing fails.
- [x] 3.4 Apply a reverse-video mouse-selection paint layer after semantic/Reading presentation but before opaque overlays, clearing selection when an overlay opens, without changing cache signatures, semantic source, or explicit span backgrounds.
- [x] 3.5 Add TestBackend UI regression tests for Transcript and Preview highlighting, pane-boundary clamping, wide grapheme cells, style preservation, and Reading View overlap.

## 4. Runtime interaction and clipboard delivery

- [x] 4.1 Wire normalized pointer gestures through `RuntimeController` to start, update, cancel, or finish frontend selection against the committed frame.
- [x] 4.2 On non-empty primary release, extract visual text and return the existing `UiAction::WriteClipboard` effect after state locks are released; retain existing success/failure toast behavior.
- [x] 4.3 Clear or cancel stale selection on focus loss, resize, session switch, new-session draft activation, history prepend, Preview target replacement, opaque-overlay activation, and committed-frame epoch mismatch.
- [x] 4.4 Add controller and scripted-port tests covering release copy success/failure, unmatched release after focus loss, empty click behavior, and Reading View complete-source copy remaining independent.
- [x] 4.5 Verify selection-only pointer updates do not structurally invalidate transcript or Preview caches and remain coalesced through the existing interactive scheduler.
- [x] 4.6 Move copy success feedback out of the composer into a final-layer popup, add an explicit expiry deadline (three seconds by default), and test that drafts and cursor rendering remain intact.
- [x] 4.7 Add the copied line count and a grapheme-safe six-character content preview to the success popup, appending `...` only when truncated.

## 5. Documentation and validation

- [x] 5.1 Update `docs/client.md` and `docs/design.md` with captured-mouse visual selection, visual-versus-semantic copy behavior, supported surfaces, and lifecycle limits.
- [x] 5.2 Update `AGENTS.md`, the UI help overlay, and README key guidance if user-visible behavior or copy instructions need clarification.
- [x] 5.3 Run focused Rust formatter, parser/runtime, selection-model, and TestBackend UI tests; run the relevant bounded-work checks and verify `cargo fmt --all --check` passes.
- [ ] 5.4 Manually verify Windows Terminal and one non-Windows SGR terminal with wheel scrolling, transcript selection, Preview selection, CJK/emoji text, streaming, resize, focus loss, and clipboard failure behavior.

## 6. Architecture refinement

- [x] 6.1 Normalize SGR modifier bits before classifying wheel/primary reports and reject pointer input when the committed viewport differs from the sampled terminal size.
- [x] 6.2 Return candidate selectable geometry as a local render artifact, keep the committed frame in the runner, and remove committed/pending selection fields from `RenderState`.
- [x] 6.3 Keep the selection reducer and Unicode extraction independent of Ratatui painting; move rendered-line collection and reverse-video painting into the UI layer.
- [x] 6.4 Return explicit selection update state and use committed-frame revision changes as the structural invalidation mechanism instead of duplicated projection clears.
- [x] 6.5 Replace the copy-specific interaction toast tuple with a frontend-owned transient notice model and route clipboard completion through the common effect-result reducer.
- [x] 6.6 Add focused regression coverage, update architecture documentation, and run scoped formatting/tests plus `cargo fmt --all --check`.
