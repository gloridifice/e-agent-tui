## Context

The Rust client currently represents configuration surfaces with four independent states. `/settings` and `/login` are rendered in the bottom two-thirds of the page in place of the input bar, while `/model` and `/theme` are border-based overlays rendered after the main frame. `main.rs` owns separate optional states and separate keyboard branches; `ui.rs` contains picker state, domain rendering, and overlay rendering; settings and login each implement their own cursor, editor, scrolling, hints, and row styling.

The change is client-only and must preserve the existing WebSocket messages, config persistence behavior, transcript render-cache semantics, and main-loop lock discipline. Configuration pages are low-frequency UI, but their rendering must remain bounded for dynamically supplied provider/model lists. Existing uncommitted project work must not be reset or overwritten during implementation.

## Goals / Non-Goals

**Goals:**

- Provide one Input Page lifecycle for settings, login, model, and theme pages.
- Enforce one active page and one focused actionable element at a time.
- Give every page the same input-replacement layout, padding, navigation keys, activation, back behavior, loading/error presentation, and focus styling.
- Keep page-specific domain rules in page-specific state while reusing focus, editor, viewport, shell, and row primitives.
- Preserve focus across asynchronous catalog refreshes when the focused logical element still exists.
- Reduce page-specific state, key routing, and rendering branches in `main.rs` and `ui.rs`.

**Non-Goals:**

- Converting the session picker, help overlay, copy mode, approval cards, or question bar into Input Pages.
- Changing bridge payloads, protocol versions, model selection semantics, credential storage, or theme file formats.
- Introducing mouse support, arbitrary key rebinding, or a general-purpose terminal form framework.
- Reworking transcript rendering, input history, or the ordinary input widget.

## Decisions

### 1. Use a closed `InputPage` enum with shared infrastructure

The client will own one `Option<InputPageSession>`. The session contains an `InputPage` enum (`Settings`, `Login`, `Model`, `Theme`), shared focus state, shared editor state where applicable, and viewport state. It exposes common `handle_key`, bridge-update, and render entry points through static enum dispatch.

A trait-object hierarchy was rejected because page event contexts differ (mutable config versus bridge effects), page rendering requires typed domain data, and object-safe abstractions would add indirection without allowing third-party page implementations. A fully declarative form schema was rejected because account login status, model columns, theme swatches, and nested proxy flows require custom rendering and behavior. The enum keeps the supported roster explicit while still centralizing lifecycle invariants.

### 2. Return declarative outcomes from page input handling

Page handlers will not await, lock shared application state, persist files, or send directly to the bridge. They return a `PageOutcome` containing a close flag and zero or more effects such as `Send(ClientMessage)` and `ConfigChanged`. The main loop applies effects after the page borrow ends.

This preserves the existing rule that mutex guards do not cross `.await` and prevents page modules from depending on terminal-loop ownership details. Settings and theme may mutate the passed client config before emitting `ConfigChanged`; the caller remains responsible for theme resolution, saving, cache invalidation, and updating `InputState` runtime fields.

### 3. Centralize the page shell and exact padding

`render_input_page` will paint the complete replacement area with `theme.bg_soft`, then derive an inner rectangle with Ratatui padding of left/right 2 columns and top/bottom 1 row. Header, optional header/body gap, body, and footer are all placed inside this rectangle. The outer padding rows and columns remain blank but retain the page background.

The page continues to use the current bottom-area policy: when an Input Page is open it receives approximately two-thirds of the terminal page, leaving the transcript visible above and status/title rows below. Page renderers must not use borders, centered overlay rectangles, or `Clear`. Existing per-row leading spaces that duplicate shell padding will be removed.

### 4. Represent navigation as one stable focus ID over an explicit focus graph

Each page supplies enabled focus nodes with stable logical IDs and explicit directional neighbors. The shared dispatcher maps `h/j/k/l` and arrow keys to `Left/Down/Up/Right`, follows the graph, and invokes the current node on Enter. Read-only, loading, informational, and unavailable elements are omitted or disabled.

Explicit neighbors were chosen over terminal-coordinate nearest-neighbor calculations because settings tabs, inline options, and model columns need deterministic movement independent of wrapping and terminal width. Stable IDs based on domain identity (setting key, provider ID, model ID, theme name, proxy ID) allow refreshed data to retain focus. If an ID disappears, the page chooses the nearest valid fallback and ensures it is visible.

The selected value and the focused element are separate concepts: selected settings choices and the active model/theme retain their `●` marker, while only the focused actionable element receives the focus background.

### 5. Distinguish browse mode from editor mode

In browse mode, arrows and `hjkl` navigate and Enter activates. Text fields use a shared editor state supporting insertion, backspace, confirmation, cancellation, and secret masking. While a text editor is active, printable characters including `h/j/k/l` are inserted rather than interpreted as navigation. Enter confirms and Esc cancels without leaking the key to the page or ordinary input.

Choice editing may expose its options as horizontal focus nodes; Enter confirms the focused option and Esc restores the prior value. Page-specific validation and application remain in settings/login domain code.

### 6. Define page-specific focus behavior

- **Settings:** category tabs are actionable focus nodes; Enter activates a category. Setting rows are actionable unless read-only. Enter on a text or choice row starts its editor. Category/item positions may be remembered, but only the shared focus is visibly active.
- **Login:** menu entries, providers, account login action, proxy creation fields/actions, and confirmation buttons are focusable when actionable. Existing proxy rows open a delete-confirmation subpage; only its Delete action sends the existing `LoginProxyDelete` message. Informational account states and non-writable provider credentials are not falsely actionable.
- **Model:** providers and models form a two-column focus graph. Activating a provider makes it the displayed provider and moves focus to its current or first model. Activating a model sends `ModelSet` and closes the page. Loading and empty states contain no fake model target.
- **Theme:** each available theme is one focus node with its swatch as decoration. Activating it updates config, persists through `ConfigChanged`, and closes the page.

### 7. Route all page state through the unified owner

`main.rs` will replace the four configuration-page options and their key/render branches with one `input_page`. `runtime_command.rs` opens the corresponding enum variant. The render argument previously named `RenderOverlays` will carry the optional Input Page separately from true overlays. Login/model server frames update only a matching active page; late frames after close or page replacement are ignored while global provider/model status updates continue as today.

Model and login catalog application methods update existing page state rather than reconstructing it blindly, allowing stable-focus reconciliation. The session picker remains an independent overlay and retains its current priority outside Input Page mode.

### 8. Keep page renderers and reusable primitives separate

The shared module will provide shell layout, focus styling, footer/loading/error rendering, text/secret fields, choice markers, list-row helpers, and visible-window calculations. Settings/login/model/theme modules retain their domain state and compose those primitives. Picker state currently embedded in `ui.rs` will move to page-specific modules so `ui.rs` primarily coordinates rendering.

## Risks / Trade-offs

- **[Risk] A common abstraction becomes a second UI framework** → Keep only shell, focus, editor, viewport, and small rendering primitives generic; retain custom page bodies and a closed page enum.
- **[Risk] Explicit focus graphs become stale after dynamic data changes** → Rebuild/reconcile the graph whenever login/model data changes, validate the active ID, and test deletion/reordering/empty transitions.
- **[Risk] `hjkl` navigation prevents entering those letters** → Route keys through the active text editor before browse-key normalization and add regression tests.
- **[Risk] Exact padding reduces already limited small-terminal space** → Use saturating layout calculations, clip body content, keep footer bounded, and test short/narrow terminals.
- **[Risk] Proxy deletion becomes easier to trigger** → Require a dedicated confirmation subpage; never send delete directly from the list row.
- **[Risk] Settings navigation changes from direct category switching to focusable tabs** → Document the new single-focus behavior and retain predictable directional links from tabs to category items.
- **[Risk] Large provider/model catalogs allocate focus nodes** → Build only lightweight logical nodes when data changes rather than every transcript frame; render only the visible body window.

## Migration Plan

1. Add shared Input Page state, event normalization, focus graph, editor, shell, and unit tests without changing command entry points.
2. Migrate settings and login to the shared owner and replacement renderer while preserving config and bridge behavior.
3. Move model and theme state out of overlay rendering and into Input Page variants.
4. Replace main-loop state, key, server-update, and render branches; remove obsolete model/theme overlay functions.
5. Add page-level and TestBackend regressions, update help and documentation, then run the complete Rust test suite.

Rollback is source-level: restore the prior page-specific options and overlay render paths. No persisted data or wire migration is required because config files and protocol messages do not change.

## Open Questions

None. The proposal fixes the initial scope to settings/login/model/theme, focusable settings tabs, and confirmed proxy deletion; further surfaces can reuse the focus utilities in later changes without becoming part of the Input Page contract now.
