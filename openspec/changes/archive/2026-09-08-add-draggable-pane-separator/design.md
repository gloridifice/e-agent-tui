## Context

The Screen currently owns a responsive `ScreenLayout` that computes the main pane as 60% of terminal width capped by an absolute `main_pane_width`, requires a 32-column Preview, and otherwise selects main-only or narrow full-screen Preview. Primary-button events already arrive as normalized press/drag/release values on Crossterm and the Windows raw-VT path, but the runtime currently sends every such gesture to application-owned text selection. Transcript and Preview caches already detect committed width changes, so the missing pieces are transient separator ownership, percentage geometry, a cheap drag presentation, and persistence on release.

The implementation must preserve the event-driven frame scheduler, committed selection-frame discipline, kernel-neutral `e-tui` boundary, shared Preview state, Reading View, and the rule that render-time code performs no filesystem I/O.

## Goals / Non-Goals

**Goals:**
- Let a primary-button drag resize the message/Preview split and persist the committed message width as a percentage.
- Enforce a 25% message minimum and a 16-column usable Preview-content threshold (19 columns for the split Preview rectangle).
- Keep drag frames cheap by rendering only responsive placeholder boxes and separator feedback until release.
- Integrate separator ownership with existing mouse selection, focus-loss cancellation, config persistence, and `Ctrl+P` Preview fallback.
- Keep geometry calculation and rendering consistent across normal frames, drag frames, runtime hit testing, scrolling, and release-time rematerialization.

**Non-Goals:**
- Changing Preview semantic target selection, content resolution, scrolling, reveal pacing, or Reading View behavior.
- Adding keyboard separator adjustment, a new bridge message, protocol field, external dependency, or filesystem access to `e-tui`.
- Rendering real transcript or Preview content continuously at each drag coordinate.
- Migrating an old absolute-column preference into a percentage using an arbitrary or remembered terminal width.

## Decisions

### Store a validated percentage as integer basis points

Replace `main_pane_width` with a persisted `message_pane_percent` value represented internally as integer basis points in the inclusive range `2500..=10000`. Serialize it as a percentage value and expose parsing/display helpers for config and Settings. Pane columns are always derived from current usable width by rounded integer percentage arithmetic; release converts the final main column back into basis points.

Basis points preserve sub-percent drag positions without floating-point drift or long serialized decimals. A whole-number percentage was considered, but terminals wider than 100 columns would make one setting step move multiple cells. Storing columns alongside the percentage was rejected because it creates two competing authorities.

The embedded default is 60%. The old `main_pane_width` key is removed from the embedded schema, so the existing known-key overlay ignores it as an obsolete unknown field and old files inherit 60%.

### Keep pending resize state in `InteractionModel`

Add a frontend-owned `PaneResizeState` with an optional drag containing the start column, whether the gesture began collapsed, and pending percentage/collapse state. Pending values never enter `Config`, `TuiApp` semantic state, Preview state, transcript cache keys, or persisted storage. A release returns a committed percentage; focus loss or resize cancels the pending gesture without saving it.

This follows the existing ownership rule for local mouse selection and other user interaction. Updating Config on each drag report was rejected because it would invalidate width-dependent content and cause repeated persistence.

### Make Screen geometry the shared source of truth

`ui::screen` continues to own pane rectangles and adds separator geometry/hit-testing helpers used by both rendering and the runtime pointer path. For a committed or pending percentage:

- main width is the rounded percentage of usable width and never below 25%;
- Preview is split only when its total rectangle is at least 19 columns: one separator column, one post-separator gap, 16 usable content columns, and one right margin;
- otherwise normal mode is main-only and the grip is placed inside the right margin;
- existing narrow `PreviewOnly` behavior remains available through `Ctrl+P` and has no separator.

A temporary collapse caused only by terminal resize does not rewrite the stored percentage, so widening the terminal restores the split. An explicit drag into the sub-19-column zone commits 100%, which remains collapsed until the right-margin grip is dragged left.

When a gesture starts collapsed, the first leftward movement restores a 19-column Preview rectangle containing 16 content columns and subsequent movement increases it continuously. If the terminal cannot fit both a 25% main pane and the 19-column Preview rectangle, the layout remains collapsed.

### Unify pane insets and preserve the separator gap

Pane-level geometry uses one-column margins as the common visual rule. The Main page reserves one column on each ordinary edge. In split mode, Preview uses two columns on its left edge because its first raw column is the separator and the second is the required blank gap; it uses one column on the right edge. Full-screen Preview has one column on both sides. Main-only mode additionally reserves the collapsed grip, its one-column gap, and the terminal's one-column right margin so message content cannot overwrite the grip.

The default `user_input_padding` is one column, matching the Main page edge and the Preview content gap. The setting remains user-editable for compatibility, so an explicit user value is not forcibly rewritten.

The separator renderer consumes `theme.separator.bar`, `theme.separator.line`, and `theme.separator.placeholder` as complete semantic styles. Their configured backgrounds are applied to the corresponding cells; when a custom theme omits a background, the fallback is the resolved themed base surface rather than a literal palette color.

### Capture separator gestures before text selection

On primary press, the runtime first checks the small separator grip hit area. A hit starts pane resize, clears any mouse selection, and captures subsequent primary drag/release reports in `PaneResizeState`, even when the pointer leaves the original grip. All other primary gestures continue through the committed visible-frame selection reducer unchanged. Wheel events retain transcript scrolling.

Separator handling occurs before the selection-frame viewport guard because its geometry comes from the current terminal size and committed percentage, not selectable content metadata. Focus loss and terminal resize cancel both resize capture and stale text selection.

### Use a dedicated cheap render branch during drag

The render input exposes the pending pane resize state to Screen. While active, Screen does not call the main Pane, Preview Pane, Reading geometry, transcript renderer, selection painter, help/content overlays, or toast rendering. It paints the full area with the base surface, then renders one or two margin-inset boxes using `theme.surface.muted_text.fg` as the Bark-equivalent background, optional ratio labels, a full-height one-cell Bark guide, and a thicker central grip. The boxes follow the pending percentage on each coalesced interactive frame.

Normal frames render existing content unchanged and add only the short Bark grip as the final Screen layer. On release, Config and committed geometry change once; the existing width-aware transcript and Preview cache paths then rematerialize content once at the new width. Bridge events and semantic reveal state may continue reducing while content is hidden, so the restored frame is current.

### Persist only the committed release

On release, `e-dsh::RuntimeController` synchronizes the validated percentage into both the runtime Config snapshot and `TuiApp.config`, then returns the existing owned `UiAction::PersistConfig`. The terminal event already requests an interactive frame. No UI lock is held during filesystem persistence, and a failed save follows the existing config-effect failure path.

## Risks / Trade-offs

- **[Old custom width preference resets]** → Document the breaking field replacement; ignore `main_pane_width` through the existing obsolete-key behavior and inherit 60% rather than inventing a terminal-dependent migration.
- **[Small terminals cannot satisfy both limits]** → Keep Preview collapsed and retain `Ctrl+P` full-screen Preview; never violate the 25% main minimum to force a split.
- **[Separator conflicts with text selection]** → Restrict capture to a small visible grip hit area and route it before selectable-frame hit testing only after an exact geometry match.
- **[Placeholder frames accidentally perform expensive work]** → Short-circuit Screen before Pane/Region rendering and assert zero transcript/Preview rebuild or patch work in TestBackend regression tests.
- **[Rounding causes a one-cell release jump]** → Use one shared basis-point conversion and rectangle helper for pending guide, placeholders, committed layout, and hit testing.
- **[Drag is interrupted by focus or resize]** → Cancel without persistence and request the normal frame; do not interpret a later unmatched release as selection or resize completion.
- **[Custom themes do not name a literal Bark palette entry]** → Use the fixed semantic muted-text foreground, which is Bark in bundled Ferra and the corresponding muted tone in every valid theme.

## Migration Plan

1. Add the validated percentage type and embedded 60% default; replace the Settings row and all layout callers.
2. Add transient resize state and shared Screen geometry without enabling gesture capture.
3. Add normal grip and drag placeholder rendering with UI/cache regression tests.
4. Route separator pointer capture and release-time persistence through the runtime.
5. Remove production references to `main_pane_width`, update docs, and verify old config keys are ignored while new values round-trip.
6. Apply the follow-up pane-inset policy: one-column Main margins, a real Preview gap after the separator, 16 usable Preview content columns, and matching placeholder/selection/cache geometry.

Rollback restores the old field/layout and removes transient resize state. User files containing the new key are safely ignored by the old known-key overlay, so rollback does not require file conversion.

## Open Questions

None. The accepted HTML prototype fixes the intended visual and interaction behavior; implementation should retain its ratio labels during drag unless UI testing shows they are unreadable at the minimum pane width.
