# Rust client

Architecture conventions for the Rust workspace. `crates/e-dsh` is package `e-dsh` and owns the `dshe.exe` artifact (with transitional library import name `e`); `crates/e-tui` is the kernel-neutral frontend library. During the staged extraction, modules not yet moved remain under `e-dsh`, and current ownership is always determined by the code rather than the target plan.

- **Current package boundary**: DSH `ServerMessage`/`ClientMessage` and raw host-event parsing remain in `e-dsh::protocol`. `e-dsh::bridge::adapter` converts inbound values to `e-tui::AgentEvent` and outbound `e-tui::AgentRequest` values back to wire messages. `e-tui::TuiApp` owns `SessionModel`, `TimelineModel`, `CatalogModel`, `InteractionModel`, `RenderState`, shared Preview state/cache, Reading Document/Layout, and Reading View state. `e-dsh::AppState` remains the normalized DSH reduction adapter around that root; it does not mirror migrated lifecycle fields. `RuntimeController` returns owned `e-tui::UiAction` values, and the runner executes or awaits them only after releasing state guards. `e-tui` contains no DSH message/event names, WebSocket, filesystem persistence, clipboard implementation, process control, or terminal lifecycle, enforced by `crates/e-dsh/tests/architecture.rs`.
- **Event display model** (`crates/e-tui/src/display.rs` + `crates/e-tui/src/projection/{store,assistant,tool,lifecycle,retry,command,workflow,surface}.rs` +
  `crates/e-tui/src/transcript_layout.rs`): all visible events fall into four public surfaces: `ActivityRow` (with
  Waiting/Running/Success/Failure/Cancelled state, optionally with parent/depth), `TranscriptBlock`
  (plain/markdown/reasoning/unknown fallback), `ContentCard` (uniform padding/background/copy source), and
  `InputAccessory` (above the input bar). Production `TimelineModel` holds the sole `TranscriptStore` and
  `EventProjector`; the projector first produces display/surface mutation/page state/accessory/ignore effects, which the
  state layer then applies; adding event-specific top-level rendering in `ui` that bypasses the public
  surfaces is forbidden. `LegacyTestMsg`/`Msg` alias may only appear in `#[cfg(test)]` characterization
  fixtures and must not re-enter production transcript, renderer, cache, or copy paths.
- **Reasoning output folding**: `TranscriptFormat::Reasoning` blocks are not rendered to screen in compact
  mode and do not enter copy provenance (production `ui/transcript.rs::is_hidden_item` makes layout/cache/copy
  skip them, without producing an inter-row gap); activity-row adjacency must look up the next **non-hidden**
  DisplayItem — hidden reasoning must not split apart activity rows that should be glued together.
  lines/full mode renders reasoning content directly: the lines cap is the **post-wrap display row count**
  (width-aware wrap happens before `thinking_lines` truncation); and whenever reasoning is visible, the
  immediately preceding `Thinking...` activity row is taken over and hidden by `thinking_row_superseded`
  (not rendered, no gap). The hidden determination must be uniform across rendering/copy/adjacency/hiding
  itself (`is_hidden_node`); animation patches skip hidden nodes and must **not** fall into the full-rebuild
  fallback. Thinking is represented by a `• Thinking... xN` breathing indicator; `assistant/chunk` carrying
  only reasoning does not settle until the real answer text arrives, which settles to green. Therefore
  Thinking settlement must search backwards in `TranscriptStore` for a Running Thinking activity — never
  assume the last node is visible.
- **Context injection text**: prompt-injection events (`CardRole::Context`) render as plain text, not a
  card shell: a `提示词注入` label in the activity label tone (umber in the ferra theme) followed by the
  injected content in the activity detail tone (bark), capped at 2 lines by post-wrap display row count;
  if it overflows, the 2nd line ends with `…`. The card's `copy_source`/copy unit must preserve the
  complete original text and must not be truncated by the display clip.
- **File activity folding**: `FileGroup` keeps the `read/view/edit/replace/insert` labels via a unified
  `FileItem + FileAction`; consecutive `str_replace_editor` view/str_replace/insert and read/edit calls enter
  the same folded activity row; the editor's absolute path is converted to a workspace-relative path using
  `session_cwd`. create does not enter FileGroup and is shown separately as
  `<indicator> create <relative-path>`, and does not append output line count/elapsed time after completion.
  All activity rows stay on a single display row; when too wide, `transcript_layout`/`ui::transcript`
  truncate and append `…` using the resolved page content width (including `page_max_width`) — never
  pre-truncate to terminal width and then wrap inside a narrower page. Generic tool rows show output line count
  and elapsed time as soon as they start; animation patches update elapsed time, and truncation reserves these
  trailing metrics by shortening the command/summary first. Known tool schemas should use readable summaries;
  grep renders as `grep "<pattern>" at "<path>"` rather than raw JSON arguments.
- **Surface semantics**: `HostEvent` parses the event top-level `time`, `surfaceOp`, `sourceEventSeqs`;
  replace must first remove the shadowed surface owner, then insert the replacement node at the original
  surface position. Unknown events that carry `surfaceOp` must also enter the snapshot/history compatibility
  path. On history prepend, save the shadowed seq so later older pages cannot revive compacted content; when a
  tool/command/Code Mode/workflow terminal half is split from its start by a page boundary, stage it and
  rebuild the final state directly when the older page's start arrives; when a retry schedule and a newer
  retry-started span a page boundary, backfill delay/failure/maxRetries after restoring saved rows — do not
  drop details just for dedup; move the viewport only by the truly newly added rendered rows. workflow
  completed/failed/cancelled must be kept as a typed outcome and mapped to Success/Failure/Cancelled.
  compaction's log-only summary is not drawn as its own card; the single summary card is created and owned by
  the replacement, so a later replace can delete it precisely.
- **Render cache** (`e-tui::{cache,transcript_layout,ui::region::transcript}`): only
  structural events invalidate the cache and trigger a full rebuild; streaming chunks only set `tail_dirty`,
  and rendering **splices the tail** and recomputes only the tail display-row suffix/prefix — never clear the
  entire layout. Paced assistant reveal records the earliest changed message and splices from that message
  through the suffix, because adding a grapheme may change wrapped/Markdown line counts; it must not fall back
  to rebuilding earlier messages. Spinner/settle only patch the active `DisplayId` range, and settle must submit
  one more precise target-color patch after expiry before stopping the clock. Semantic Reading geometry and
  `ProvenanceLayoutRow` values come from the same width/generation layout; Reading cursor movement and Preview
  selection must not flatten or rebuild the transcript. Wrap scanning is greedy line wrapping: rows fill
  with whole words until the next word no longer fits, the break consumes the separating whitespace, and a
  word wider than the row falls back to grapheme splitting. A word is a maximal run with no Unicode Line
  Breaking Algorithm (UAX #14) break opportunity between its graphemes: Latin words, glued punctuation
  pairs, and Hangul syllable blocks stay whole, while CJK ideographs, kana, and Hangul syllables each
  offer a break opportunity (so kinsoku punctuation never starts a row). Display width is computed by
  Unicode grapheme cluster; combining marks / emoji ZWJ must not be split even across style spans.
- **Paced text reveal** (`e-tui::reveal`): semantic transcript/Preview content is always complete; only
  presentation sidecars hold paint progress. `TEXT_FADE_WEIGHTS` is the single newest-to-oldest static profile
  (currently `[0.217, 0.53]`), and each affected foreground is
  `background_color + (semantic_fg - background_color) * weight`; backgrounds and modifiers are preserved.
  Live assistant Markdown remains grapheme-paced at `message_chars_per_second` (default 120), but admits only a
  stable rendered tail prefix: the shared UAX #14 wrapper retains the open trailing atom/deferred separator until
  a later break, 100ms rendered-idle timeout, 300ms absolute timeout, or stream settlement. Completed hard-wrap
  rows of an over-wide atom remain eligible. Ready Preview is first wrapped and then row-paced at
  `preview_lines_per_second` (default 30); a unit is a non-empty terminal display row, not a source line or
  grapheme. Preview resize retains its semantic grapheme frontier while recomputing current row boundaries.
  The first admitted grapheme/row may appear immediately; higher rates reveal `ceil(rate × 16ms)` units as one
  visible batch and delayed deadlines advance at most one batch. Admission, content reveal, and a 16ms fade clock
  have independent deadlines. Fade groups continue to restore original foregrounds even when an open stream has
  no queued content, then become deadline-idle until new content arrives. Markdown control syntax and structural
  line boundaries consume no transcript budget; semantic printable whitespace and Unicode grapheme clusters do,
  while generated code-block fill padding is applied after clipping and never enters the signature. Replay/history
  never starts transcript reveal, copy and Reading use complete source, plain-color mode keeps pacing but omits
  interpolation, and Preview identity changes restart only Preview while same-target revisions retain the common
  semantic prefix.
- **Performance red lines** (all have regression tests): terminal input wakes the main loop directly through
  `EventStream` — do not restore fixed ticker polling; interaction/content/animation deadlines are separated,
  spinner, transcript reveal, and Preview reveal keep independent due times whose minimum wakes the loop; each
  due reveal lane advances at most one visible batch, and the bridge backlog is bounded per turn by a count+time
  budget. On Windows, crossterm's record-based event source never emits `Event::Paste`, so
  `ProductionTerminalEvents` (`runtime_ports.rs`) reads the raw VT byte stream instead: `TerminalOwner` enables
  `ENABLE_VIRTUAL_TERMINAL_INPUT` only after ratatui/crossterm terminal construction (that setup clears an earlier
  flag), a reader thread forwards stdin byte chunks together with an immediate physical Shift/Ctrl/Alt/Backspace
  snapshot, and `vt_input.rs` parses them into crossterm events (bracketed paste, navigation, SGR mouse scroll, Alt
  prefixes). Terminals that consume `Ctrl+V` still deliver bracketed paste; terminals that pass it through produce
  a modified key, which the router turns into `UiAction::ReadClipboard`. The `e-dsh` clipboard port reads through
  `arboard`, completion reuses the same active-editor/composer paste path, and both sources normalize CRLF/lone CR
  through `e-tui::input::normalize_paste_text`. Reading View suppresses both paste forms, and a non-editing Input
  Page must not mutate the preserved composer draft behind it. Windows Terminal's measured byte table is Backspace
  `0x7f`, **Ctrl+Backspace `0x17` (ETB)**, Ctrl+H
  `0x08`, Alt+Backspace `0x1b 0x7f`; re-measure with `cargo run -p e-dsh --example input_probe` before changing
  Backspace handling instead of assuming an encoding. The snapshot recovers `\r` Enter modifiers and identifies a
  real Backspace origin before the reader-to-async handoff can make `VK_BACK` stale: `0x17` with physical Backspace
  becomes Ctrl+Backspace and otherwise stays Ctrl+W, `0x08` stays Ctrl+H when Ctrl is held without physical
  Backspace so the help binding keeps working, and `0x08`/`0x7f` gain a Ctrl modifier only on terminals that do
  report both. Kitty CSI-u and `modifyOtherKeys` preserve their encoded modifiers. Escape/partial-sequence deadlines
  are stored on `WindowsRawInput`, not in one cancellable
  `next_event()` future; otherwise frame or bridge wakeups can restart the timeout forever and swallow Esc. Keep
  that raw-input path between the stream and the router, or pasted `\r` line endings commit/send the message at
  every newline. The terminal is initialized/restored at a single point via
  `terminal_runtime.rs::TerminalOwner`; frames are committed atomically with a 64KiB
  `BufWriter` + DEC 2026 synchronized output (`DSHE_DISABLE_SYNC_OUTPUT=1` only as a compatibility diagnostic).
  Selectable geometry is an ephemeral render artifact: `e-tui` returns a candidate visible-row frame, the runner
  replaces its local committed frame only after terminal submission succeeds, and neither pending nor committed
  selection geometry enters `RenderState` or semantic cache state. Never full-render per event; redraw P95 ≤30ms
  and only when dirty/deadline expires; animation only patches
  the active message range, streaming only splices the tail; display-row layout is cached by width/generation,
  and each frame only materializes/clones the visible window. Do not break the shared layout semantics of
  `valid/tail_dirty/dirty_messages`, history display-row anchor, and copy provenance.
- **Runtime controller / lock discipline**: `runtime.rs::RuntimeController` receives typed `RuntimeInput`,
  consumes `ControllerAction` inside a single scoped guard, and hands only complete-payload `RuntimeEffect`s
  to the `main.rs` executor; `runtime_ports.rs` provides transport, terminal, config/state, clipboard read/write, and
  clock production/scripted ports. The executor must not borrow UI state or silently ignore effects; do not
  restore a fixed ticker. In Rust 2021, `if let`/`match` scrutinee temporaries live until the end of the whole
  expression; never write `state_r.lock()` directly into a scrutinee and then re-lock or `.await` in a branch,
  or you will self-deadlock. Compute plain values/actions in a separate scope before matching, or perform
  atomic state changes within a single guard; `main.rs` already denies `clippy::significant_drop_in_scrutinee`
  and has queue-dispatch/copy-mode lock-release regression tests.
- **Frontend interaction ownership**:
  `e-tui::{catalog,command_catalog,input,page_core,input_page,login,settings,question,interaction}` owns composer
  state, catalog presentation/completion, Input Page focus/editing, login/settings page state, retained question
  batches, approval routing, scroll/follow, help, transient notice deadlines, mouse-selection reducer state, and
  prompt queues. Question, approval, and queued-prompt
  state is session-scoped and must be cleared together on a bridge welcome that switches session identity. The old
  executable-side re-export facades have been removed; protocol DTO conversion and external action execution remain
  in `e-dsh`.
- **Input interaction and character boundaries**: `InputState.cursor` is a **character index**;
  `String::insert/remove` and slicing need byte indices — use `char_to_byte()` (`input.rs`); CJK has regression
  tests; cursor x uses `unicode_width`. Plain input is fixed: `Enter` sends, `Shift+Enter` inserts a newline, and
  `Ctrl+V` requests an application clipboard read when the terminal does not already translate it into bracketed
  paste. Input Pages accept either paste source only in their active text editor; modified shortcut letters are not
  inserted as literal text. Over-threshold composer pastes become **independent atomic paste blocks**
  (`InputState.paste_blocks`, raw-buffer char
  ranges): each renders as one Rose `[N text pasted]` placeholder between ordinary editable text, ←/→ skip a
  whole block, Backspace/Delete remove the whole block, Up/Down map the cursor through the placeholder
  (snapping into a block to its start), Enter sends the full expanded content verbatim, and history-browsing
  drafts keep their blocks. `Ctrl+Backspace`/`Ctrl+W` (Windows) and `Alt/Option+Backspace` (macOS) delete the
  word before the cursor together with the whitespace around it (Windows textbox style). Ctrl+W shares this path
  because Windows Terminal encodes Ctrl+Backspace as that byte and the snapshot cannot always prove the origin.
  A paste block is one
  atomic unit, both whitespace scans stop at block boundaries, Han ideographs and kana delete one Unicode
  grapheme per press (identified through Unicode Script_Extensions, including halfwidth/extended kana), and
  letters/digits/`_` grapheme runs and symbol runs are single units. External text restoration (`restore_text`)
  cannot infer paste identity and yields plain text. The suggestion popup never opens while a paste block exists
  (a fill would destroy the block).
  `↑/↓` move between input lines by character column first, and only switch to the previous/next history prompt
  at the first/last line boundary; `PageUp`/`PageDown` page by the currently visible transcript height, and the
  mouse wheel moves 3 lines per notch (always operating on the transcript even when an Input Page is open).
  A primary-button press on the pane separator is captured by resize before selectable-content hit testing;
  captured separator drags update only transient geometry, clear any existing selection, and remain resize-owned
  when they cross Transcript or Preview cells. Other primary-button drags select only visible Transcript or Preview
  text in the surface where the drag starts, use display-cell/grapheme boundaries, and copy rendered visual rows on
  release; this partial visual copy is distinct from Reading View `y`, which copies the complete owning source Block.
  The pure selection reducer
  reads only the immutable runner-owned committed visible frame; frame revision or viewport mismatch rejects stale
  paint/copy after resize, session/draft/history/Preview identity changes, while focus loss and new primary press
  are explicit cancellation transitions. Ratatui line collection and reverse-video painting stay in
  `ui::selection`, outside the selection kernel.
  `Ctrl+H` is a global help key handled before the Input Page, and `hjkl` with Control/Alt/Super must not enter
  the focus graph. `Config.enter_sends` exists only for legacy config deserialization compatibility and must
  no longer change key semantics. The terminal hardware cursor must always be hidden inside the TUI; the screen
  only draws a software reverse-video cursor; `ui.rs::render_with_cursor` only returns the IME anchor, and the
  main loop moves the hidden cursor after the frame completes. Do not call `Frame::set_cursor_position` again —
  it makes ratatui show and drag the cursor during diff drawing, causing the status light/input bar to flicker.
- **Overlays and Input Page rendering**: a command prompt that truly draws over the transcript must first call
  `frame.render_widget(Clear, rect)` before drawing the background, otherwise underlying text bleeds through
  (there is a test `suggest_popup_is_opaque_over_transcript`). `/settings` `/login` `/model` `/theme` `/resume`
  are not overlays: they are uniformly handled by `InputPageSession` replacing the input area, no border, no
  `Clear`, with the shared shell fixed at 1 row top/bottom and 2 columns left/right padding. Within the Ash shell,
  settings paints the full category strip and right-side value pane with the base (Night) surface; the strip is
  display-only, while value edit focus on the Night pane uses the panel (Ash) surface for contrast.
- **Reading View and copy semantics**: `Ctrl+Y` enters Reading View (`Ctrl+V` remains reserved for paste);
  `Ctrl+P` toggles the existing full-screen Preview fallback when the responsive split is too narrow; Preview-only
  presentation has no pane separator. Block mode uses
  `j`/`k`, `l`, `y`, and `Esc`; Item mode uses spatial `h`/`j`/`k`/`l`, with `Esc` returning to Block mode. `y`
  always copies the complete owning Block from `ReadingCopyPayload`, never clipped terminal cells. Mouse drag
  copies only the selected visible rendered range and is intentionally separate from this complete-source
  operation. Clipboard completion is reduced into a frontend-owned generic transient notice; a successful copy
  uses a small top-layer popup (three seconds by default) showing the copied line count and a grapheme-safe
  six-character content preview, with an ellipsis only when truncated, without replacing the composer draft or
  cursor. The composition root executes clipboard I/O and schedules the exact notice deadline but does not format
  localized notice text. Tables/code/Mermaid remain atomic through stable render-unit provenance, and render unit
  ids are reused across re-renders (`unit_start`). Row-oriented Copy Mode and `Ctrl+B` no longer exist.
- **Markdown syntax and localized backgrounds**: headings directly use the active fixed Markdown semantics;
  existing `semantics.markdown` mappings remain theme-authored. Fenced code uses the embedded `syntect` grammar
  selected by its normalized fence token: syntax scopes map back onto Markdown semantic roles, transferring only
  foreground/bold/italic/underline while the containing `code_background` remains authoritative. Transcript
  fences use `semantics.markdown`; complete Markdown rendered in Preview uses the role-identical
  `semantics.markdown_weak`, whose bundled mappings use only Bark, Umber, and Night equivalents. Inline code `bg`
  may only apply to the chip span; `render_transcript` only lets `Line.style.bg` trigger full-line fill — never
  infer a full-line background from an arbitrary span's background, or you will pollute source separator spaces
  and trailing whitespace. Changing these styles must sync the built-in theme TOML, `render.rs`, the syntax scope
  adapter, and TestBackend regression tests.
- **Table cells**: must go through `cell_spans()` (`render.rs`) for inline rendering + display column-width
  truncation/padding — never stuff bare strings in.
- **Inline styling is event-driven**: `render.rs::InlineBuilder` owns the single inline implementation (chips,
  strong/emph/strikethrough, link text + underlined URL, HTML as text) and is fed the events of the block being
  parsed; `collect_inlines()` is the thin wrapper for blocks that hold their own source (paragraph, heading,
  quote, table cell). **Never re-parse a block's flattened text**: by then the parser has consumed the markers, so
  chips, emphasis, link URLs, and escapes vanish and a leading `1.`/`#` in the text is eaten as a block marker —
  exactly the bug list items had. `render_list` therefore feeds each item's own events into its builder,
  `SoftBreak::Space` keeps a source line break a word separator (the item wraps for itself), an item's
  `End(Paragraph)` starts a new row so loose items do not run together, and every open list level hands out its
  own ordered numbers (`ListLevel`), so the source's `3.` starts at 3 and a nested list cannot reset the outer
  level back to bullets. `InlineBuilder::finish()` trims trailing plain whitespace only — whitespace that carries
  a background is chip padding and must survive.
- **Width-aware Markdown layout**: `RenderOptions.content_width` is the resolved display width of the surface
  being painted (transcript cache width, or the padded Preview content width). Tables fit their columns to it;
  list items wrap against it inside `render.rs::ListRenderer` so continuation rows carry a hanging indent of
  `2 * depth + marker width` and stay in the item's text column; `render_quote` wraps quote lines the same way
  but re-emits `│ ` per level on every wrapped row so the gutter stays contiguous. Every wrapped row keeps its
  logical row's `raw_line`. `None` means "unresolved": logical rows are emitted whole and the paint-time wrapper
  (`wrap::wrap_line`) stays authoritative. `MarkdownLayoutRegistry` re-materializes a block when the source or
  `content_width` changes, so a resize re-wraps lists, quotes, and tables before the transcript cache is rebuilt.
- **History paging**: `min_seq`/`history_loading`/`history_exhausted`; prepend goes through `prepend_events`
  (sets `prepend_line_anchor`, and the renderer shifts `scroll.offset` by the truly newly added display rows to
  keep the viewport). The top "history" hint row is **display-only** and does not enter the cache; Thinking is a
  public `ActivityRow`, but is not generated during snapshot replay/history prepend (`state.replaying`), and
  file-group merge/settlement scans skip it.
- **Tracy/timing** (`profile.rs`): instrument with `e::tracy_zone!("literal")` (a macro that safely no-ops when
  no client is present); use `PhaseTimers` for stage timing. Zone names must be string literals.
- **Responsive Screen, Preview, and pane resizing**: the Screen derives message width from the committed
  `message_pane_percent` basis-point setting (default 60%, inclusive range 25.00%–100.00%) at the current terminal
  width. Preview remains beside it only when its raw rectangle is at least 19 columns: one separator column, one
  post-separator gap, 16 usable content columns, and one right margin. Otherwise normal mode is main-only with a
  short Bark-equivalent grip in the right margin, while the existing `Ctrl+P` fallback occupies the full screen
  without a grip. Main content uses one ordinary horizontal edge column; Main-only mode additionally reserves the
  collapsed grip geometry. Split Preview uses the separator plus one gap on the left and one margin on the right.
  A separator press captures the primary-button gesture before text selection; pending width is transient, clamps
  the message pane at 25%, collapses Preview below the 19-column rectangle threshold, and restores a 19-column
  Preview rectangle with 16 usable content columns on the first leftward movement from the collapsed margin grip.
  Drag frames paint only margin-inset Bark placeholder boxes, a full-height thin guide, and a thick grip; they do
  not render real panes, Reading geometry, selection, toasts, or transcript/Preview cache work. Release commits and
  persists the percentage once; focus loss or terminal resize cancels without changing it, so a temporary
  responsive collapse reopens when the terminal grows. Normal mode follows the latest semantic Block; Reading View follows Item then Block. Preview
  has independent scroll, visible-row materialization, one shared semantic cache, request-id/key/revision stale-result
  checks, a width/theme-aware styled-layout cache, and a selected-target reveal sidecar that never enters semantic
  cache keys or invalidates the transcript. Stable redraw/reveal/scroll frames reuse styled syntax rows; key/revision,
  width, or theme-style changes rematerialize only Preview layout while preserving the semantic reveal frontier. The
  remaining `e-dsh::preview_resolver` deferred path is retained for legacy file/line references and returns an event
  completion without holding a UI lock.
- **Structured tool Preview**: known tool calls carry a provider-neutral `PreviewContent::Tool` seed built at the DSH adapter boundary (the renderer never inspects DSH tool names or argument keys). The layout is a `theme.activity.label` tool-name header, the primary content on the next row with no blank row between, then — only when secondary content exists — one blank row and the secondary. read/view show a workspace-relative `path[:lines]` location (`start-end` for a window, `start-` for open-ended, `N` for a single line); create shows its path; filesystem search shows a quoted query and an optional `at "path"` row; command/bash/pwsh (Preview name `bash`/`pwsh`, or `cmd`/`powershell`/`sh`/`shell` when that is the tool name; `command` is the fallback) show a Coral `$` + Mist command row and a Bark `lines N, duration X.Xs` metrics row; unsupported tools show bounded pretty JSON under the original tool name. A command's settled `tool/result` enriches the same `tool:<call-id>` target with final line count/duration and a Bark/Umber two-tone terminal secondary (ANSI-colored runs map to Bark, uncolored runs to Umber, bold/italic preserved, every other control stripped via a `vte`-backed component). read/view/create/search/generic results stay primary-only. edit/replace/insert render event-supplied mutation fragments (DSH edit `meta.diffs`, str-replace `old_str/new_str`, addition-only insert) as removed/added rows — the client never reads a file or computes a diff. It may classify event-authored unified rows and coordinates, preserve an optional event path, and highlight old/new logical code streams independently. Diff syntax foregrounds use normal Markdown semantics; added/removed backgrounds, accents, gutters, and separators remain under `semantics.diff`. Injected context (`CardRole::Context`) previews as `MutedMarkdown`, distinct from `Reasoning`; both render through the structured `markdown_weak` hierarchy rather than a post-render forced foreground.
- **Rendering layers**: production rendering lives in `e-tui` and points downward as `Screen -> Pane -> Region -> Component`. The main pane retains the characterized transcript/composer/status style; Preview reuses theme semantics without changing main-pane tokens. Terminal setup, restoration, synchronized output, and frame scheduling stay in `e-dsh`.
- **Bottom layout and two-line status bar**: the fixed bottom row order is input bar or Input Page / gap /
  status line 1 / **session title line** (the `ui.rs::render` chunks array; the `+3` in the accessory budget
  formula matches it). Neither line sets a background color: line 1 is, left to right, the working indicator,
  `SessionModel.current_mode`, the current model, `CH<cache-hit %>`, and the reasoning-effort label
  `Effort:<Label>` (after CH), where the model, CH, and effort entries are omitted entirely when they have no
  value yet (no placeholder dash) — the effort entry is hidden unless the exact current route exposes reasoning
  metadata, resolves `current.reasoningEffort` then `reasoning.defaultEffort` then `Default`, and never
  invalidates the transcript cache. The right side is fixed `^h Help`; line 2's
  left side is `SessionModel.session_title` (shows `新会话` when empty) and the right side is the absolute
  `SessionModel.session_cwd` path, with the title truncated with `…` when too long so the path is preserved.
  mode's initial value comes from `welcome.mode` (most recent selection, else the creation header), then is
  updated by `agent-preset/selected` replay, keeping the latest value by event seq (history prepend must not
  regress it); CH accumulates from assistant usage input/cache read/cache write, where history prepend may add
  older totals but must not replace the latest request's usage anchor; these page-state updates must **not**
  touch `TranscriptRenderCache`. When changing the bottom row count, sync the hardcoded line numbers in the UI
  layer tests.
- **Command paradigm** (`runtime_command.rs` + `input.rs`): commands are split into built-in optimized commands
  and DSH integrated commands. All built-ins are declared exactly once in `BUILTIN_COMMANDS` (name/description/
  input hint/completion strategy/action in one entry) — never maintain a parallel name table in `input.rs`;
  `match_command_catalog` merges the `CommandInfo` sent by the bridge, with built-ins winning on name
  collision. Integrated commands come from each agent's effective `ctx.commands.list` view, and at minimum
  support fuzzy name completion and show DSH's free-form input hint; DSH currently has no typed argument
  completion schema, so only built-ins can do argument completion like `/new `; `/skill` is another built-in
  argument completion — typing the full `/skill` shows the current user-invocable roster and fills candidates as
  `/skill:<name>`. On receiving a new `commands`/`skills` frame, refresh any open prompt immediately; on session
  switch, clear the old agent-scoped catalog first. Generic execution must not pre-`start_thinking`; the result
  is projected directly to System/Error by `command-result`.
- **Startup and deferred `/new`**: a new process sends hello without `resumeSessionId`, and the bridge still
  creates a session in place (`hello.cwd` workspace + `hello.mode` default mode, falling back to standard on
  failure); only a CLI session id and "remember last session" (default off) resume. `/resume` opens the resume
  Input Page, `/resume <id>` attaches directly. A bare `/new` while interactive only creates a client-side
  `NewConversationDraft` (display name `新对话`) — it does not send to the bridge or replace the real session
  id/TranscriptStore; only the first plain input sends the atomic `new-input{mode,text}` to create and deliver.
  During the draft, old-session frames keep reducing but are not displayed, and the Preview pane is cleared and
  held empty (the draft page must not inherit the previous session's preview, and late old-session frames must
  not repopulate it); on create failure restore the input; `/model` and `/effort` stay usable during the draft;
  the provider/model catalog is session-independent and a selection made during the draft is applied to the
  materialized session through `/new`'s provider/model/reasoningEffort mirror, while `/skill` and integrated
  commands must not be
  misrouted to the old session.
- **Input Page controller** (`input_page.rs` + `settings.rs` + `login.rs`): the main loop holds a single
  `Option<InputPageSession>` with the closed variant set Settings/Login/Model/Effort/Theme/Resume/Question; page keys only
  return `PageOutcome`/`PageEffect`, and the caller saves or `.await`s sending only after releasing the page borrow
  and state lock. Browse-state arrow keys and `hjkl` share a stable focus graph, Enter executes; settings is the
  deliberate horizontal exception: `←`/`→` and `h`/`l` switch category pages directly, and its category strip
  never enters the focus graph. Text-edit-state `hjkl` must be ordinary characters. `ask_user_question` opens
  Question directly in this shell and its tool call/result
  are suppressed from transcript activity: `h`/`l` or `←`/`→` changes the question, `j`/`k` or `↓`/`↑` moves option
  focus, Space selects without advancing (and toggles options for multi-select questions), Enter advances/submits,
  and closing the page restores the untouched ordinary input buffer. Dynamic
  login/model/session rosters reconcile focus by
  provider/model/proxy/session id, and an empty list must not fabricate a fake focus; Resume always uses plain
  characters (including hjkl) for title/id filtering, with only ↑↓ selecting a session.
- **/login page**: a one-level two-choice menu (API key / Proxy) → sub-pages (Menu / Providers / ApiKey /
  ProxyList / ProxyForm / ProxyDelete). State comes from bridge `login` frames; the API key is never sent back
  and is drawn as ● when editing; non-writable providers must not receive action focus; an existing proxy must
  enter the delete confirmation page on Enter, and `login-proxy-delete` is only sent after explicitly choosing
  delete.
- **Config/theme/launcher (Rust boundary)**: the `Config`/theme value schemas and defaults live in `e-tui`; config defaults live only in `crates/e-tui/assets/default_config.toml`, embedded and parsed by `e-tui::config` via `include_str!`. `e-dsh::config` owns platform paths, config/state file reads and writes, and theme discovery/installation; `e-tui` performs no filesystem I/O. The persisted `Config` is deserialized directly with
  `Deserialize` + `#[serde(deny_unknown_fields)]`; validated transparent values keep `background_color` as
  `#RRGGBB` and both reveal rates in `0..=1024` (zero disables pacing and exposes complete content immediately),
  while `resolved_theme` is a `#[serde(skip)]` runtime cache. The embedded default theme is `ferra`; `deepseek-e`
  remains available as a built-in theme.
  `from_user_toml` first recursively `overlay_known`s user values onto the embedded TOML as the schema, then
  strictly deserializes exactly once: old files inherit missing fields, deprecated unknown keys are ignored,
  malformed/known-type errors fall back safely; `Config::default()` must not re-derive from Rust field literals.
  `Config.theme` stores the theme name; `message_pane_percent` is the sole persisted pane-width authority, defaults
  to 60.00%, and is validated to 25.00%–100.00%; pane columns are derived from the current terminal width.
  `user_input_padding` defaults to one column so user cards and the composer match the one-column Main page edge,
  while an explicit Settings value remains supported. The
  obsolete `main_pane_width` key is ignored by the known-key overlay rather than migrated without a terminal width;
  rendering does zero disk reads. Themes are two-layer TOML: an open
  `[colors]` allows arbitrary color names, and fixed `[semantics.*]` (surface/markdown/markdown_weak/diff/input/
  working_status/log/activity/card/overlay/separator) link semantic styles to color names; each style requires only `fg`,
  with `bg`/`bold`/`italic`/`underline` optional; unknown references, missing fixed fields, or illegal hex reject the
  whole file. An optional `padding` field on a style adds backgrounded spaces on both sides of that element: a
  scalar (`padding = 1`) sets both sides, or a table (`padding = { left = 2, right = 1 }`) sets each side
  independently. Padding defaults to zero when omitted and is opt-in per element: inline code consumes it, while
  code blocks and Mermaid ignore `code_background.padding` and keep their own fixed layout. `markdown_weak` has exactly the `markdown` role set and is required: older custom themes must add it
  or the existing whole-theme fallback applies.
  Built-in `deepseek-e`/`ferra` sources are in `crates/e-tui/assets/themes/`, embedded via `include_str!` and parsed by
  the same parser as user files, and copied without overwrite to `%APPDATA%\dshe\themes\`; a valid same-named user
  file wins, and an illegal old file must not shadow the embedded fallback. `launcher.rs`: `probe(url)` TCP probe
  → if no dsh, spawn `dsh --profile e --no-open` (`dsh` or `npx @deepseek-ai/dsh`) → `%DSH_HOME%\e.lock` counts
  "close dsh when the last tui closes"; on Windows the child handle points at the `cmd /C` shim, and both normal
  shutdown and startup-timeout cleanup must `taskkill /T` the whole process tree — never only `Child::kill`,
  which leaves orphan Node processes; child reaping must be bounded, and on terminate failure keep an
  `instances: 0` lock for the next attach to retry; reading any lock must re-`probe(url)` — even `instances > 0`
  is not proof of a live service (a force-killed TUI leaves a stale positive-count lock), and if the service is
  gone, clear the lock and rebuild. `dshe clean` bypasses setup/TUI startup, force-stops only the DSH process
  recorded in this lock, and then removes the lock; missing, malformed, zero-pid, and dead-process locks are
  removed as stale, while a lock is retained if its live process cannot be terminated. An externally started DSH
  has no project lock and is not stopped. A spawn error, child exit before readiness, or startup timeout must fail the
  launcher immediately with the attempted command and actionable setup guidance (run `dshe setup` and restart DSH)
  — never continue to token
  read/WebSocket connect and expose a raw connection-refused error. The WebSocket upgrade retries only transient I/O
  races briefly. `release` returns `true` only when it actually shut down a managed service, and after the main
  program exits the alternate screen it prints `dsh 服务器已关闭。`. The launcher must use the dedicated `e`
  profile and must not reuse DSH's own / user's existing `tui` profile (whose terminal UI grabs stdio and does not
  provide the `webServer` the bridge depends on). Hello-terminal bridge errors (`protocol-newer`, `bad-token`,
  `hello-failed`) must become actionable fatal client errors before the following WebSocket close can overwrite them
  with a generic disconnect; the client must also reject a differing protocol version in `welcome`, and protocol
  mismatch guidance must mention updating/rebuilding the client, running `dshe setup`, and restarting DSH.
  `/reload` re-reads config + rescans themes.
- After adding interaction keys, sync `e-tui/src/ui/overlay.rs`, the README quick reference, the terminal binding gate, and input/router tests.
