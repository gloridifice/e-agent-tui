## 1. Percentage Configuration

- [x] 1.1 Add a validated basis-point message-pane percentage value with 25%–100% parsing, serialization, width conversion, and column-to-percentage helpers in `e-tui::config`.
- [x] 1.2 Replace the embedded `main_pane_width` default with `message_pane_percent = 60.0`, update Config tests for inheritance, validation, round-trip persistence, and obsolete-key filtering.
- [x] 1.3 Replace the Settings absolute-column row with an immediately applied percentage row that retains the last valid value after invalid input.

## 2. Shared Resize State and Geometry

- [x] 2.1 Add frontend-owned `PaneResizeState`/drag data to `InteractionModel`, including begin, expanded update, 25% clamp, sub-19 rectangle collapse, collapsed-left restoration, release commit, and cancellation transitions.
- [x] 2.2 Refactor `ui::screen` layout to derive rectangles from the committed percentage, use a 19-column split rectangle for 16 usable Preview columns, preserve responsive percentages across terminal resize, and retain `Ctrl+P` Preview-only fallback.
- [x] 2.3 Add one shared separator geometry and hit-test policy for split and right-margin collapsed handles, including the small-terminal case that cannot satisfy both pane limits.

## 3. Separator and Placeholder Rendering

- [x] 3.1 Render the short Bark-equivalent separator grip as the final Screen layer during normal split/main-only presentation and omit it in Preview-only presentation.
- [x] 3.2 Thread pending resize presentation through the render input without putting it in semantic, Preview, transcript-cache, or selection-frame state.
- [x] 3.3 Add the drag-only Screen branch that paints the base surface, responsive margin-inset Bark boxes and ratio labels, a full-height thin guide, and a thicker grip while skipping real Pane, Region, Reading, selection, and toast rendering.
- [x] 3.4 Update render call sites, examples, and fixture overlay constructors for the new resize presentation input while keeping non-drag output unchanged.

## 4. Pointer Routing and Commit

- [x] 4.1 Give separator press/drag/release capture priority over committed-frame text selection in `RuntimeController`, clear prior selection on capture, and preserve ordinary selection and wheel behavior outside the grip.
- [x] 4.2 Cancel captured resize without persistence on focus loss or terminal resize and ignore unmatched later releases.
- [x] 4.3 On captured release, synchronize the committed percentage into runtime and application Config exactly once and return the existing owned `PersistConfig` action without holding a UI lock during I/O.
- [x] 4.4 Pass resize state between the runner-owned `InteractionModel`, terminal route state, and render transaction while retaining event-driven interactive frame scheduling.

## 5. Regression Tests

- [x] 5.1 Add pure config/layout/resize-state tests for percentage rounding, 25% clamping, exactly-16-usable-column visibility, below-19-rectangle collapse, right-margin restoration, temporary responsive collapse, and impossible small widths.
- [x] 5.2 Add TestBackend UI tests asserting the normal Bark grip, unchanged real content, drag placeholder margins/colors/labels, full-height guide, thick grip, collapsed single-box presentation, and restored release content.
- [x] 5.3 Assert drag-only frames perform zero transcript/Preview rebuild, patch, and materialized-row work, while release causes at most the expected one width rematerialization per visible pane.
- [x] 5.4 Add runtime tests proving separator ownership blocks text-copy effects, captured movement survives crossing selectable surfaces, cancellation does not persist, release persists once, and wheel/text-selection behavior remains intact.

## 6. Documentation and Validation

- [x] 6.1 Update `docs/client.md`, `docs/design.md`, and `AGENTS.md` in English with percentage sizing, limits, collapse/restore semantics, pointer priority, placeholder-only drag rendering, persistence, and cache discipline.
- [x] 6.2 Run the scoped `e-tui` config/screen/UI tests and scoped `e-dsh` runtime pointer tests required by this change.
- [x] 6.3 Run `cargo fmt --all --check` and verify the OpenSpec change remains apply-ready.

## 7. Pane Insets and Preview Gap Follow-up

- [x] 7.1 Define shared pane-inset constants: one-column Main margins, a split Preview left inset containing the separator plus one gap, a one-column Preview right margin, and a 16-column usable-content/19-column rectangle threshold; update resize and layout tests.
- [x] 7.2 Apply the inset policy to `ui::screen`, Main page geometry, drag placeholders, Preview wrapping/cache widths, Paragraph padding, and Preview selection registration; default `user_input_padding` to one column.
- [x] 7.3 Add TestBackend and pure geometry regressions for the blank column after the separator, Preview content origin, one-column Main margins, collapsed grip reservation, and exactly-16 usable Preview content.
- [x] 7.4 Synchronize English project documentation and run the scoped UI/config/interaction tests, formatting, diff checks, and OpenSpec validation.

## 8. Theme-driven Separator Backgrounds

- [x] 8.1 Route idle grip, drag guide, and placeholder fills through the corresponding `theme.separator` semantic styles, with themed base-surface fallback only when a role omits `bg`.
- [x] 8.2 Restore explicit built-in theme separator backgrounds and add TestBackend/theme regressions proving custom separator backgrounds are rendered.
- [x] 8.3 Synchronize English theme documentation and run scoped theme/separator tests, formatting, diff checks, and OpenSpec validation.
