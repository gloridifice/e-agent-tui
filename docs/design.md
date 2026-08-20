# DSH TUI Design Document (draft v0.5)

> Status: D1–D30 implemented; later implementation revisions follow this document's current sections and the
> machine-readable protocol contract.
> v0.4 changes: message format spec (user messages verbatim, shell card spinner+line count, read merge-fold);
> borderless input-bar background block + paste placeholder.
> v0.5 changes (v0.1.0 milestone): project renamed **e** (executable **`dshe`**); config moved to
> `%APPDATA%\dshe\config.toml`, theme directory `%APPDATA%\dshe\themes\` (default **deepseek-e**, plus built-in
> ferra); added `/theme` `/model` `/reload` `/skill:<name>`; the `dshe` launcher auto-spawns
> `dsh --profile dshe` (or npx) / bridges to an already-running dsh; uses the dedicated `dshe` profile to avoid
> conflicts with DSH's own or the user's existing `tui` profile. The Windows service started by `dshe` terminates
> the `cmd /C` shim's full process tree with `taskkill /T` both on startup-timeout cleanup and when the last TUI
> closes, to avoid orphan Node processes; reaping wait is bounded, and on close failure a zero-instance lock is
> kept for the next attach to retry; all instance locks must re-probe the bridge endpoint — even a positive
> instance count does not prove the service is alive, and when the service is gone the stale lock is cleared and
> rebuilt. `dshe clean` force-stops the project-managed PID in `%DSH_HOME%\e.lock` and removes that lock, but does
> not stop an externally started DSH service that has no project lock. After a confirmed shutdown it leaves the
> alternate screen and prints `dsh 服务器已关闭。`. It does not print this when the DSH was started externally by the
> bridge or when other TUIs remain.
> v0.6 architecture convergence: the production client module graph is auto-checked as SCC-free by
> `crates/e-dsh/tests/architecture.rs`; the transcript only stores public Display surfaces; wire shape/fixture/doc are
> synced from the same JSON contract; config uses a single strict schema; DSH model selection is installed via the
> public upstream adapter and verified by the deployed-copy upgrade check.

## 0. Decided decisions (✅)

| # | Decision | Conclusion |
|---|------|------|
| D1 | Run shape | standalone client process (`dshe`, project name **e** / e tui), connected to a running DSH via a bridge, coexist with the Web GUI |
| D2 | Layout | responsive main conversation pane plus full-height Preview at wide widths; main-only/full-screen Preview fallback when narrow |
| D3 | Client language | Rust (ratatui + crossterm + tokio-tungstenite + serde) |
| D4 | Project composition | TS bridge plugin (DSH side) + Rust client |
| D5 | Themes | two built-in themes deepseek-e (default) and ferra; every valid toml under `%APPDATA%\dshe\themes\` is selectable |
| D6 | Tool call card | inline single-line card (folded by default, expandable) |
| D7 | User message prefix | `❯` symbol |
| D8 | Markdown rendering | full rendering in the first phase: headings/bold-italic/inline code/code blocks/lists/quotes/**tables**/**mermaid** |
| D9 | Mermaid rendering | use grok-mermaid (WASM, from xAI Grok CLI / Simon Willison extracted build) |
| D10 | Reading View | semantic Block/Item navigation over the canonical transcript, with cursor-driven Preview and complete-source copy |
| D11 | Block copy semantics | tables, mermaid, code blocks are copied as a whole (copy original markdown source) |
| D12 | Reading keys | `Ctrl+Y` enters (selected after the `Ctrl+V` paste gate failed); `j/k/l/y/Esc` in Block mode and spatial `hjkl` in Item mode |
| D13 | Overlong atomic block | over threshold (default 40 lines) may fold; mermaid diagrams handled separately |
| D14 | Syntax highlighting | syntect in phase two; first-phase code blocks plain color + language label |
| D15 | Bridge auth | lightweight token: the bridge plugin generates a random token written to the DSH data directory; the client reads it automatically |
| D16 | Bridge plugin shape | a proper TS plugin package (may use the `ws` library), maintained long-term as part of the product |
| D17 | Startup behavior | each new `dshe` process creates a new session by default; only a CLI session id or "remember last session" (default off) resumes; `/resume`/Ctrl+N opens the resume Input Page |
| D18 | Mermaid over-width | v1 truncate + fold hint (copy still gets full source); v2 full-screen graph mode with hjkl four-direction scroll |
| D19 | Copy hint | after a successful copy, the input area briefly shows `已复制 N 行`, disappearing after ~2 seconds |
| D20 | User message display | shown verbatim character-for-character, no markdown rendering; prefix `❯` Coral |
| D21 | Command execution tool card | breathing bullet + command + live output line count and elapsed time; success/failure color the bullet Sage/Ember; over-width commands truncate before the trailing metrics so those metrics remain visible |
| D22 | read merge and fold | adjacent reads within the same turn merge into a compact status list; when all finish, fold into Bark gray `<a>, <b>, <c>` (filenames only, over-width truncates `+N`); Enter expands back, Esc collapses |
| D23 | Input bar shape | borderless background block: Ash base, 1-row top margin + text area + 1-row bottom margin; prefix `❯` Coral; `Enter` fixed send, `Shift+Enter` newline; `↑/↓` move between lines and switch prompts at first/last line boundary |
| D24 | Overlong paste placeholder | paste over the config threshold shows Rose `[N text pasted]`; sends the full content verbatim; plain text inserts newlines with `Shift+Enter` |
| D25 | Spinner configurable | default A half-moon rotation `◐◓◑◒` (~120ms/frame); frame sequence made a configurable enum (`config.toml` can switch B/C/D/E); missing glyphs degrade to ASCII `\|/-\` |
| D26 | Input Page | `/settings` `/login` `/model` `/theme` `/resume` uniformly replace the input area (not a floating window); 1-row top/bottom, 2-column left/right padding; single focus moves with arrows/`hjkl`, `Enter` executes, `Esc` returns |
| D27 | Config storage | the sole source of defaults is `crates/e-tui/assets/default_config.toml` (embedded via `include_str!` and parsed); `%APPDATA%\dshe\config.toml` is an overlay allowed to omit fields; priority embedded defaults < user file < runtime; **save immediately, take effect immediately** |
| D28 | In-TUI editable items | see §4.7 list: appearance/behavior/display are all editable, advanced is read-only |
| D29 | Not editable in TUI | connection parameters (startup flag), font size (terminal side), clipboard backend (platform), key rebinding (v2), syntax highlighting theme (phase two) |
| D30 | Send key semantics | fixed `Enter` send, `Shift+Enter` newline; the legacy `enter_sends` config only keeps deserialization compatibility and no longer changes interaction |

## 1. Goals and shape

- The TUI is a **second frontend** for the DSH Web GUI: it reuses the same session log and event stream, rendered
  to the terminal.
- Single-session view: one terminal window focuses one session; multiple sessions switch through a selector.
- Design principles:
  1. the message stream is the protagonist; UI elements avoid obscuring content as much as possible;
  2. keyboard first, mouse (wheel/click) as enhancement;
  3. while the agent is running the user can still type (messages queue), no need to wait;
  4. **AI output is rich content**: markdown/tables/mermaid render fully, and copying gets the original source,
     not rendered fragments.

## 2. Overall architecture

```
┌─────────────────────┐        WebSocket         ┌──────────────────────┐
│  DSH process         │  ws://127.0.0.1:PORT/   │  dshe (e, Rust proc) │
│  TS bridge plugin    │ ◄────────────────────► │  ratatui rendering   │
│  · session/event     │   JSON protocol (§5)    │  · msg flow / input  │
│  · agent/status      │                          │  · markdown+table    │
│  · commands.execute  │                          │  · mermaid (WASM)    │
│  · approval/question │                          │  · copy (source map) │
└─────────────────────┘                          └──────────────────────┘
```

Key fact: `webServer.registerUpgrade(path, handler)`'s handler receives Node's native
`(req: IncomingMessage, socket: Duplex, head: Buffer)` — the plugin performs the WebSocket handshake and frame
handling itself. The bridge layer may use a proper TS plugin + the `ws` library (recommended), and could in theory
also hand-write RFC6455 as a dynamic plugin (see O8).

### 2.1 Client rendering pipeline (core architecture, supporting D10/D11)

```
markdown source
  │  pulldown-cmark parse (preserve each block's source range / raw text)
  ▼
block sequence: Paragraph / Heading / CodeBlock / Table / Mermaid / List / Quote ...
  │  each block → rendered as a RenderUnit carrying source metadata
  ▼
RenderUnit { kind, source: { blockType, raw: String }, cells: RenderedCells }
  │  raw = the block's complete source text in the original markdown
  ▼
screen buffer: each rendered line records its owning RenderUnit (line → block map)
  │
  ▼
Reading View: semantic Block/Item id → shared layout/provenance geometry → complete Block copy payload
              Preview target follows latest Block or Reading cursor without duplicating transcript text
```

- Copy always takes the **original markdown source**: tables copy out the `| a | b |` pipe source, headings copy
  out `## heading`, mermaid copies out the ` ```mermaid ... ` fenced source.
- The provenance map is maintained incrementally with rendering; Reading navigation does not re-parse markdown.
- Tables/mermaid/code blocks are **atomic blocks**: `y` copies the complete source regardless of wrapping or the selected Item.

### 2.2 Dependency direction, runtime, and projection boundary

Client production modules obey one-way dependencies, continuously guarded by the source edge scanner, Tarjan SCC
check, and forbidden-reverse-edge assertions in `crates/e-dsh/tests/architecture.rs`:

```text
main (Tokio composition root)
  ├─ runtime / runtime_ports ──> command_catalog / page_core
  ├─ input / runtime_command ──> command_catalog
  ├─ input_page ──> page_core + settings/login/model/theme/resume
  └─ ui + copy ──> transcript_layout ──> display/render/config

protocol (typed DTO + HostEvent family parser)
  └─ projection/{assistant,tool,lifecycle,retry,command,workflow,surface}
       └─ TranscriptStore (only the DisplayItem public surface)
```

`RuntimeController` receives already-typed bridge frames, terminal events, and deadlines, and while holding a
short-lived state lock only produces internal actions or a complete `RuntimeEffect`. `main.rs` only handles
`tokio::select!`, bounded inbound, deadlines, terminal lifecycle, and the effect executor; transport, terminal
events, config/state files, clipboard, clock, and the launcher's processes/locks are all adapted through narrow
ports, so scripted stand-ins can verify races without awaiting/I/O inside the lock. The launcher treats process
spawn failure, exit-before-readiness, and readiness timeout as explicit startup failures rather than continuing
with a stale token into a raw WebSocket connection error. The transport briefly retries transient upgrade races;
wire compatibility is then checked in both directions (the bridge rejects a newer client, and the client rejects a
mismatched `welcome.protocolVersion`) with one same-checkout update/`dshe setup`/rebuild/restart recovery path.

Production `AppState` stores only `ActivityRow`, `TranscriptBlock`, `ContentCard`, and composite `DisplayItem` in
`TranscriptStore`. `EventProjector` and the family projections are the only HostEvent→display entry point;
`LegacyTestMsg` exists only in `#[cfg(test)]` characterization fixtures and cannot re-enter the production
renderer, cache, or copy path. `transcript_layout` is the width/generation/provenance layout kernel shared by UI
and copy, so tail splice, activity range patch, history anchor, and original-Markdown copy use the same line
semantics.

## 3. Visual design

### 3.1 Overall layout

```
┌──────────────────────────────────────────────────┐
│ • standard deepseek-v4-pro CH80%     ^h Help │ ← bottom status line 1 (no background)
├──────────────────────────────────────────────────┤
│ ❯ Refactor the foo function                       │ ← user message: shown verbatim
│                                                  │
│ 🤖 Let me read the file first… (streaming)        │ ← message stream (scrollable, main)
│    ◐ npm run build · 128 lines                    │ ← command card: spinner + command + live count
│    ◐ reading                                     │ ← consecutive reads merged into a compact list
│      src/foo.ts   ✓ read                         │
│      src/bar.ts   ◐ reading                      │
│      src/baz.ts   … queued                       │
│    (all done →) <src/foo.ts>, <src/bar.ts>, <…>  │ ← folded into a gray single line, filenames only
│    Here is the data:                             │
│    ┌──────────────────────────────────────────┐  │
│    │ colA    colB    │  ← markdown table (box)  │  │
│    └──────────────────────────────────────────┘  │
│                                                  │
│ ⚠ approval · allow writing src/foo.ts?           │ ← approval card (fixed above input)
│    [Y] allow   [n] deny   [i] details            │
├──────────────────────────────────────────────────┤
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ ← input bar: borderless background block
│▓ ❯ type a message…                  [Ctrl+H help]▓│    top/bottom margin rows + text area (≤3-line scroll)
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│    Reading View preserves this complete draft state
└──────────────────────────────────────────────────┘
```

### 3.2 Role visual language (ferra 256 color)

| Role | Visual | Notes |
|------|------|------|
| User message | Coral `#ffa07a` prefix `❯`, body Mist | **shown verbatim, no markdown rendering** |
| assistant text | body Mist `#d1d1e0`, 1-column Sage `#b1b695` bar on the left | distinct from tool cards |
| Command execution card | spinner (default half-moon rotation `◐◓◑◒` ~120ms/frame, configurable) Honey; done `✓`Sage / `✗`Ember | only shows command + live output line count |
| read merged list | indented 2 columns; `◐`Honey=reading `✓`Sage=read `✗`Ember=failed `…`Bark=queued | folds to a gray single line when done |
| read folded message | Bark `#6f5d63` gray, `<a>, <b>, <c>` filenames only | Enter expands back, Esc collapses |
| Other tool cards | Bark inline card, indented 2 columns | `●`Honey=running `✓`Sage=success `✗`Ember=failed |
| system / hints | Bark `#6f5d63` | session start, compaction hints |
| Errors | Ember `#e06b75`, `✗` prefix | agent/error, tool failure |
| Approval card | Honey `#f5d76e` bordered block | details expandable |
| Link/emphasis | Blush `#fecdb2` / Rose `#f6b6c9` | markdown inline |
| Paste placeholder | Rose `#f6b6c9` `[N text pasted]` | long paste folded in single-line mode |
| Input bar | Ash `#383539` background block, no border; prefix `❯` Coral | top/bottom margin rows + text area |
| Reading selection | current Block uses Night plus a Bark gutter rail; current Item uses a local selection background | explicit inline backgrounds remain authoritative |

### 3.3 Message rendering spec

#### 3.3.1 User messages

- Show user input text verbatim, **no markdown parsing**; prefix `❯` Coral.
- Message block is top/bottom margin rows + text area, horizontal padding (gutter, default 2 columns) applied to
  **every line** — when over-wide text wraps to the page width, continuation lines keep the same gutter.
- Over-long pasted content is still sent as-is; the display layer handles it per the §4.1 placeholder rule.

#### 3.3.2 Tool message format spec

**Command execution class (shell/bash/pwsh etc.)** — D21

```
• npm run build · 0 lines · 0.0s       ← running: breathing Honey bullet; metrics exist immediately
• npm run build · 128 lines · 11.2s    ← exit 0: Sage bullet
• npm run build · 64 lines · 3.4s      ← exit non-0: Ember bullet
```

- Output body is not shown while running. The line count starts at zero and updates when output is projected; the
  elapsed time updates on animation ticks. Command text stays on one row and truncates with `…` at the centered
  page's actual content width (including the "page max width" setting). Truncation reserves the complete trailing
  `· N lines · N.Ns` metrics instead of clipping them with the command.
- Tool summaries use compact readable forms where a schema is known; for example grep is
  `grep "<pattern>" at "<path>"` rather than raw JSON arguments.
- Success/failure is carried by the Sage/Ember bullet color.
- Expand (Enter): view the full command and output/stderr (with the fold rules below).

**File read class (read etc.)** — D22

```
◐ reading                        ← group head: spinner + group status
  src/foo.ts    ✓ read
  src/bar.ts    ◐ reading
  src/baz.ts    … queued
────────────────────── (folds to, after all done) ──────────────────────
<src/foo.ts>, <src/bar.ts>, <src/baz.ts>    ← Bark gray, filenames only
```

- Merge rule: **adjacent** read-class calls within the same turn (no assistant text, no other tool type between)
  form one group.
- The group head shows a spinner while any item in the group is unfinished; a single-file read uses the same list
  shape (1 item).
- The folded line truncates when over-wide and shows `+N` (e.g. `<a>, <b>, <c> +2`); Enter expands back, Esc
  collapses.
- Failed file: `✗` Ember + filename; expand to see the error details.

**Other tools (write/edit/search etc.)**: keep the D6 inline single card — `●` Honey running → `✓` Sage / `✗`
Ember, format `✓ tool-name arg-summary · elapsed`.

#### 3.3.2.1 Structured tool Preview

The Preview pane presents known tool calls through one `PreviewContent::Tool` layout, with a blank row only
before the optional secondary:

```
tool_name                       ← Umber (semantics.activity.label)
primary_content                 ← tool-specific semantic colors

optional secondary_content      ← tool-specific semantic colors
```

| tool | Preview name | primary | secondary |
|---|---|---|---|
| read / view | `read` / `view` | workspace-relative `path[:lines]` (`N`, `N-M`, or `N-`) in Mist | — |
| create | `create` | path in Mist | — |
| search (grep/glob) | `search` | `"query"` in Mist, optional `at "path"` in Bark+Mist | — |
| command / bash / pwsh | `command` / `bash` / `pwsh` | `$` Coral + command Mist; `lines N, duration X.Xs` Bark | command output, two-tone |
| other (unknown schema) | original tool name | bounded pretty JSON in Mist | — |

- The two-tone terminal secondary maps ANSI-colored output to Bark and uncolored output to Umber, preserving
  bold/italic and stripping every other terminal control (a `vte`-backed component, never raw escape bytes).
- Command tools keep their real name on both surfaces: the transcript activity label and the Preview header show
  `bash`, `pwsh`, `cmd`, `powershell`, `sh`, or `shell` when the tool name is one of those, and fall back to
  `command` for any other Command-capability name (e.g. a third-party `run_command`). This replaces the old
  behavior that labeled every shell tool `command` in the transcript.
- A settled command result enriches the same `tool:<call-id>` target (metrics + secondary); read/view/create/search
  and generic results stay primary-only and never grow an output body.
- edit/replace/insert render event-supplied mutation fragments, never a client-computed diff:
  the DSH `edit` tool persists applied contextual hunks in result `meta.diffs` (`{ path, oldText, newText }`);
  `str_replace_editor.str_replace` exposes call-time `old_str/new_str` (a requested hunk, retained after
  settlement because DSH supplies no applied result hunk); `insert` exposes only `new_str` + `insert_line`, so it
  renders as an addition-only hunk anchored to that line. `create` stays common-format even though it carries a
  full `file_text`.
- Prompt-injection/context cards preview as `MutedMarkdown` (full Markdown with every foreground forced to Bark),
  distinct from the `Reasoning` kind that Thinking uses.

#### 3.3.3 Markdown and rich content

- Markdown: headings, bold, italic, inline code, fenced code blocks (language label + plain color; syntect still
  later), ordered/unordered lists, quotes, separators, **tables** (box-rendered, adaptive column width, over-width
  truncation annotation), **mermaid**. Headings directly use `semantics.markdown.heading1..6`; under ferra, level 1
  is Coral `#ffa07a` bold with no background, level 2 is Sage `#b1b695` bold, level 3 is Blush `#fecdb2` non-bold.
- Inline code: the background is strictly limited to the inline-code chip itself (including chip padding); the
  following separator spaces in the source and unused trailing cells keep the normal line background. Full-line
  background fill only recognizes line-level `Line.style.bg`, never inferred from a local span.
- Code blocks: left vertical border + top language label; v1 wraps over-width, horizontal scroll v2.
- mermaid: grok-mermaid WASM renders to a Unicode diagram; on render failure degrade to the source fence block
  (copyable).
- Long content fold: plain long text / tool results > N lines (default 20) fold, showing head + tail +
  `… [Enter] expand`. (Atomic blocks table/mermaid/code don't fold; see O12 for whether there's an exception.)
- Streaming: token-level append; auto-follow the bottom when not scrolled up; scrolling up pauses follow and shows
  a `↓ new messages` indicator.

### 3.4 Two-layer theme system and ferra palette

Theme TOML has two layers:

1. `[colors]` is an open palette — key names are entirely up to the theme author, values must be 6-digit hex;
   the runtime does not depend on fixed names like `night`, `ok`.
2. `[semantics.*]` is the fixed semantic schema, containing `surface`, `markdown`, `input` (including the status
   bar), `working_status`, `log`, `activity`, `card`, `overlay`. Each fixed role is a style object with only `fg`
   required; `bg`, `bold`, `italic`, `underline` optional; color values reference user color names in `[colors]`.

```toml
[colors]
night = "#2b292d"
blush = "#fecdb2"

[semantics.markdown]
heading3 = { fg = "blush" }
inline_code = { fg = "blush", bg = "night" }
```

A missing fixed semantic field, an unknown semantic field, a reference to a nonexistent color name, or an illegal
color makes the whole theme file invalid and it is skipped from the theme directory. The built-in `deepseek-e` and
`ferra` are also not hardcoded Rust palettes: their source files live in `crates/e-tui/assets/themes/`, compiled in via
`include_str!` and parsed by the same parser; on first load they are copied verbatim to
`%APPDATA%\dshe\themes\` without overwriting existing user files. A valid same-named user theme takes priority
over the embedded version, and an illegal old file does not shadow the built-in fallback. The parsed fixed
semantic styles are cached in `Config.resolved_theme`; rendering reads no disk.

The ferra palette comes from the casperstorm/ferra README:

| Name | Hex | Use (client) |
|------|-----|----------------|
| Night | `#2b292d` | client background / code block and inline code base |
| Ash | `#383539` | card/input bar/user message block base |
| Umber | `#4d424b` | selection base (atomic block selection) |
| Bark | `#6f5d63` | secondary text / tool cards |
| Mist | `#d1d1e0` | body foreground |
| Sage | `#b1b695` | assistant bar / success / level-2 heading (bold) |
| Blush | `#fecdb2` | links / level-3 heading (non-bold) |
| Coral | `#ffa07a` | user `❯` / user highlight / level-1 heading (bold, no background) |
| Rose | `#f6b6c9` | emphasis / inline code |
| Ember | `#e06b75` | error / failure |
| Honey | `#f5d76e` | running / approval card / warning |

- Terminal side: the official Windows Terminal color scheme ([ferra ports/windows terminal](https://github.com/casperstorm/ferra/tree/main/ports/windows%20terminal)).
- True color (24-bit) primary; degraded environments fall back to ferra's nearest 256-color index.
- Supports `NO_COLOR` / `--no-color` plain-text mode.

### 3.5 State presentation

- The page bottom has two fixed status lines, neither with a background color. The first line starts with the
  status symbol `•` (consistent with tool cards: yellow breathing while running, gray when idle), then the current
  agent preset mode, the current model, and `CH<cache-hit %>`; when the current model or CH has no value yet the
  whole entry is omitted, no placeholder dash. CH accumulates from provider usage
  `cacheRead / (input + cacheRead + cacheWrite)`. History prepend adds older usage totals but keeps the latest
  request's replacement anchor; mode's initial value comes from `welcome.mode` (most recent selection, else the
  creation header), then keeps the latest value by `agent-preset/selected` event seq — old pages must not regress
  it. The right side is fixed `^h Help`.
- The second line shows the current session title on the left (shows `新会话` when untitled) and the session
  workspace absolute path on the right; over-long titles truncate with `…`, prioritizing the path.
- Reading View leaves the composer draft untouched and routes navigation through the central input owner; exiting restores the same buffer, cursor, multiline mode, and completion state.

### 3.6 Resume Input Page (/resume / Ctrl+N)

- Like other Input Pages, it replaces the input area rather than overlaying the transcript: typed text filters by
  title or id, ↑↓ selects, Enter resumes, Esc returns.
- The page first shows a loading state; the bridge's `session-list` boundary sends `sessions{titlesPending:true}`
  as soon as the persisted header list completes, then folds titles for at most 200 candidates and sends the final
  `sessions`. So slow disk title reads do not block the list's first screen.

### 3.7 Help overlay (? / Ctrl+H)

- Half-screen overlay: all shortcuts for the current mode; q/Esc closes. It lists Reading Block/Item navigation and the narrow Preview toggle.

## 4. Interaction design

### 4.1 Input model (D23/D24)

**Shape: borderless background block** (Ash `#383539` base, no border lines):

```
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓   ← top margin 1 row (pure background)
▓ ❯ type text…                 ▓   ← text area: single-line mode 1 row
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓   ← bottom margin 1 row (pure background)
```

- Text area: single-line mode 1 row; multi-line mode shows at most **3 rows**, scrolling within the area when
  content overflows, keeping the cursor's line visible; over-wide content **auto-wraps** within the input bar
  (the wrap window also follows the cursor). The screen cursor is drawn by the input bar as a reverse-video block;
  the terminal hardware cursor is always hidden and only moved to the same position after each frame completes as
  the IME anchor, to avoid flickering between the running status light and the input bar during diff drawing.
- Prefix `❯` Coral, consistent with the user message prefix; input text Mist.
- **Paste placeholder** (bracketed paste): pasted content > **64 characters** → the input bar shows a Rose
  `[N text pasted]` **paste block**, content not expanded; the paste block is atomic — the cursor cannot enter its
  interior (←/→ skip the whole block), Backspace/Delete delete the whole placeholder content; pressing Enter sends
  the content verbatim and complete.
- **Pre-send queue**: prompts sent with Enter while the AI is running are not sent immediately but enter the client
  queue, shown line by line above the input bar (Night base Bark text, left-indented 2 spaces with a `* ` prefix,
  one per line, over-wide `…` truncation; the row count is bounded by panel height, overflow shows `… N more`).
  When the AI returns to idle they are **sent one by one automatically** (each send starts a new turn); `Esc`
  interrupt clears the not-yet-sent queue; switching sessions also clears the queue. Prompts sent while idle still
  go out immediately. In the client implementation, dequeuing the head and starting Thinking must happen within
  the same short-lived state lock, released before the WebSocket `.await`, so auto-dispatch does not self-deadlock
  the input loop.
- Input keys are fixed `Enter` send, `Shift+Enter` insert newline; `↑↓` move between input lines keeping the
  character column, and only switch to the previous/next history prompt when the cursor is already on the top/
  bottom line; `Ctrl+R` search history, `Tab` complete. `PageUp`/`PageDown` page by the currently visible
  transcript height, and the mouse wheel moves 3 lines per notch; both always scroll the message stream even when
  an Input Page is open. `Ctrl+H` is a global help key handled before page dispatch, and modified `hjkl` do not
  participate in page focus navigation.

### 4.1.1 Command paradigm: built-in and integrated commands

Commands uniformly use `/name [raw input]` interaction and the same completion popup, but split into two classes
by compatibility depth:

1. **Built-in commands**: commands dshe has interaction-optimized. `crates/e-dsh/src/runtime_command.rs`'s
   `BUILTIN_COMMANDS` is the only registry; one entry declares name, description, DSH-style input hint, argument
   completion strategy, and action together. Registering an entry registers both behavior and completion;
   `input.rs` must not maintain a second command table. Currently `/settings`, `/login`, `/new`, `/resume`,
   `/model`, `/theme`, `/reload`, `/skill`, `/compact`, `/goal`, `/plan`, `/copy`, `/clear`, and exit aliases are
   built-ins; `/new ` does argument-level completion from the bridge's preset roster; typing the full `/skill`
   switches to the current session's user-invocable skill roster, continuing to fuzzy-filter by name and filling
   the canonical `/skill:<name>`.
2. **Integrated commands**: DSH native commands or commands registered by other DSH plugins. The bridge calls
   `ctx.commands.list(agent)` after attach to get the agent's effective catalog (global definitions +
   agent-scoped shadow), sends a handler-free `commands` frame, and recomputes per connection on `commands/change`
   rather than requiring a dshe release. The client merges with the built-in catalog (built-ins win on name
   collision) and uniformly completes by prefix→substring→subsequence fuzzy matching. Execution still sends
   `command{line}`; the bridge calls `commands.execute` and shows the direct UI outcome as System/Error via
   `command-result`; unknown commands error out and never degrade into a model user message.

DSH 0.1.0-rc.6's public [`CommandDescriptor`](https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/commands)
only has name, description, and optional `input.hint` (free-form text), with no typed argument completion schema.
Therefore integrated commands all support **command-name completion** and show the argument hint; argument
candidate completion is only available for items promoted to built-in optimized commands. Command execution is
async; the bridge must capture the current conn before the await and verify the connection is still attached to the
same session before returning the result. Each execution also gets a dedicated `AbortController`; the client tracks
unsettled direct commands so Esc sends `interrupt` even while the agent status itself remains idle, and the bridge
aborts both those command signals and any active agent turn.

### 4.2 Keybinding table (v1 proposal)

| Key | Function | Notes |
|----|------|------|
| Enter | send input-bar message | single/multi-line consistent |
| Shift+Enter | input newline | |
| ↑ / ↓ | move between input lines; switch prompts at boundary | keep character column |
| Ctrl+R | reverse history search | |
| Tab | command completion | |
| Ctrl+C | clear non-empty input; exit when idle and empty | does not interrupt work |
| Ctrl+L | redraw | |
| PgUp / PgDn | page by currently visible transcript height | scrolling up pauses auto-follow |
| Wheel | scroll the message stream 3 lines per notch | still only scrolls the stream when an Input Page is open |
| Esc | interrupt an active turn/direct command; otherwise Input Page back/close or close popup | owning surface takes precedence |
| Arrows / hjkl (Input Page) | move the single focus | in text-edit state hjkl is text |
| Enter (folded card) | expand/collapse tool result | focus navigation v2 |
| Ctrl+N | resume Input Page | input filter, ↑↓ select |
| /settings | settings panel (§4.7) | save immediately |
| ? / Ctrl+H | help overlay | |
| **Ctrl+Y** | **enter Reading View** | D12; `Ctrl+V` failed the universal paste-delivery gate |
| **Ctrl+P** | **toggle full-screen Preview on narrow terminals** | wide terminals keep Preview visible |

### 4.3 Reading View and semantic copy (D10/D11/D12)

Reading View indexes the canonical transcript and shared width-aware provenance; it does not replay events into a
second message store. Entry selects the eligible semantic Block nearest the viewport center while preserving the
complete composer draft. Night marks the current Block and a Bark rail occupies the existing outer gutter without
changing wrapping. Item highlights are local and never overwrite explicit span backgrounds.

| Key | Function |
|----|------|
| j / Down, k / Up | next / previous Block; in Item mode move spatially and cross Block boundaries |
| l / Right | enter Item mode or move to the next spatial Item |
| h / Left | move left; at the boundary return to Block mode |
| y | copy the complete owning Block source, even when an Item is selected |
| Esc | Item mode → Block mode; Block mode → normal composer |

Tables, Mermaid, and code retain atomic complete-source payloads, including original fence/pipe syntax. Resize,
streaming, settlement, history prepend, and theme rematerialization preserve semantic ids; only geometry is rebuilt.
Clipboard and deferred Preview effects execute after the UI guard is released.

### 4.4 Approval and user questions

- The approval card is fixed above the input area and does not block the message stream (background keeps
  rendering).
- Approval: `Y/n/i` or `←→` + Enter; when the Web GUI answers first the card disappears automatically with a hint.
- User questions (ask_user_question, single/multi-select batches) open in the shared Input Page shell; their tool
  call/result events do not enter transcript activity. `h`/`l` or `←`/`→` switches between questions while retaining
  each answer; `j`/`k` or `↓`/`↑` moves the option focus. Space selects the focused option without advancing; on a
  multi-select question it toggles that option independently. Enter advances to the next question, and Enter on the
  last question submits the whole batch; Esc cancels it. Questions without preset options degrade to text editing where
  `hjkl` remain ordinary text (`←`/`→` still switch questions). Opening and closing
  the page never mutates the ordinary input buffer, so any draft prompt reappears unchanged after the batch finishes.
  When the Web GUI answers/aborts first (`question/resolved`) the page disappears automatically. The bridge forwards
  the API proxy mux's RPC-enveloped `question/requested`/`question/resolved` frames, and answers return via
  `apiProxy.respond`'s `client-response` — Web and TUI can both answer, and the host takes the first to arrive.

### 4.5 Interrupt semantics

- `Esc` interrupts the active agent turn and every direct DSH/plugin command currently executing for the attached
  connection. Direct commands remain interruptible even when the agent reports idle because they use separate
  execution signals. The client keeps the command active until `command-result` or a command error/cancellation
  acknowledgment arrives.
- `Ctrl+C` clears a non-empty input buffer. With an empty buffer it exits only while no turn or direct command is
  active.

### 4.6 Session management

- Startup: **each new TUI process creates a new session by default** (one screen per process, one session each) —
  the client sends `hello` without `resumeSessionId` and the bridge creates in place: workspace takes `hello.cwd`
  (the TUI launch directory), mode takes `hello.mode` (the /settings "default mode" preset id; on failure the
  bridge falls back to `standard`, then the roster default); `dshe <session-id>` or enabling "remember last
  session" (default off) instead resumes the specified/last session. **Resume is cold-session friendly**: when the
  id is not in the active registry (inevitable after a host restart), the bridge first recovers the persisted
  session via `sessionPersistence` + `agents.resume` using the session record's preset (`agent-preset/selected`
  event > header); if it cannot recover (not persisted / preset deleted) it creates a new session — **never
  disconnect because "session not active"**. Only a create failure (missing agents etc.) ends with an error frame +
  4001.
- `Ctrl+N` / `/resume` opens the resume Input Page; `/resume <session-id>` directly attaches to switch (the page/
  `/resume` also resumes first for cold sessions, and only errors when not found — never disconnects). The session
  list excludes blank sessions by DSH's `turn/start` boundary, and filters before applying the 200-entry cap;
  active sessions read in-memory events, cold sessions prefer the `sessionListMetadata.blank` projection/cache and
  then fall back to the typed persistence log, failing open on classification errors. The filtered list first sends
  headers and titles directly obtainable from online logs, then asynchronously fills persisted titles;
  `sessionQuery.readTitleSnapshots`'s settled result must be unwrapped from `fulfilled.value.title.title`, never
  mistaken for a flat `{sessionId,title}`.
- Below the status bar there is a fixed line showing the current session (no background): title on the left,
  workspace path on the right. The title is initially filled by `welcome.title` (the session log's most recent
  `session/title`, read by the bridge at attach); cold-resumed session logs are not in memory, so the bridge
  re-sends a `title{title}` frame via `sessionQuery.readTitleSnapshots`; afterwards `session/title` events update
  in real time through ordinary event frames (the client only updates that line, without rebuilding the transcript
  cache). The path is filled by `welcome.cwd` (the session header `header.cwd`, read by the bridge at attach);
  over-long titles truncate with `…` to preserve the right-side path, empty titles show `新会话`, empty paths leave
  the right side blank.
- `/new`: first create a **client-only draft**, showing an empty transcript and the display name `新对话`, but keep
  and continue reducing the background already-attached real session; do not send `/new`, do not fake/persist a
  session id. The first plain input goes through the atomic `new-input{mode,text}` to make the bridge create/attach
  a session and `followup` the new agent; on create failure restore the full input and keep the draft. The new
  session lands in the workspace of the **TUI launch directory** — the client sends `cwd` in `hello`, and the
  bridge uses it (after validating it as a real directory) as `agents.create`'s `meta.cwd`, then `attachSession`s
  the new session into that cwd's workspace ledger (consistent with host `session.create`'s two steps); when the
  client sends no cwd (older clients) it falls back to the current session header's cwd / `process.cwd()`. A bare
  `/new` takes `config.default_mode`; an explicit `/new <mode>` overrides once. Before the draft materializes,
  `/resume` is usable, but `/model`, `/skill`, and integrated commands must not be misrouted to the old session.
  The bridge still keeps the older-client `/new` handler and mounts the preset in `agents.create`'s `setup`.
- `/new <mode>`: create a new session by agent preset id (standard/code/minimal/cordis and user-built presets).
  The bridge sends a `presets` roster frame (id/name/description/order/broken) after each attach; the client pops
  up the mode prompt when typing `/new ` (with a trailing space) (same ↑↓/Tab/Enter/Esc semantics as command
  prompts, fuzzy matching by id/display-name prefix-substring-subsequence, broken presets not sent). Unknown modes
  error with the available ids listed.
- **model selection**: every session the bridge creates/resumes first installs the public
  `@deepseek-ai/dsh-agent@0.1.0-rc.6` package-root `installModelSelection(agentCtx, selection) -> disposer` via the
  `bridge/src/model-selection.js` adapter in `setup`. It injects `variables.{provider,model}` in
  `system-prompt/assemble`, and that assembly snapshot routes `agent/request`, avoiding a missing persona
  `{{model}}` variable. `/new` mirrors the current session's provider/model, otherwise takes
  `agentDefaultModel.currentSelection()`; adapter install and preset mount are two orthogonal steps, the former
  running first. The bridge maintains no local waterfall copy; `bridge/package.json` precisely declares the
  verified agent peer, and after a DSH upgrade you must run `npm run verify-dsh-upgrade`, which checks the
  contract, host/agent version/export, and the deployed copy's `/new`, cold resume, and `/model` assembly/request
  routing.

### 4.7 Input Page and settings pages (D26–D30)

- **Unified scope**: `/settings`, `/login`, `/model`, `/theme`, `/resume` are managed by a single
  `Option<InputPageSession>` mutually exclusive. They are not overlays: no floating window, no border, no `Clear`,
  but **replace the input bar and take 2/3 of the page height**, with the message stream kept above.
- **Common shape**: Ash background; all content has fixed 1-row top/bottom and 2-column left/right blank padding; a
  common header/body/footer provides title, body, loading/error, and key hints. Only the currently executable
  element uses the Night focus background, and the currently selected value is marked with a green `●`.
- **Common keys**: arrows and `hjkl` move between executable elements in a stable focus graph, `Enter` executes,
  `Esc` cancels editing/returns/closes; read-only, loading, info, and disabled elements do not gain focus.
  Text-edit state consumes characters first, so `hjkl` types normally rather than navigating. Dynamic
  provider/model/proxy/session rosters keep focus by stable id; `/resume`'s filter box is always in text-input
  state and only uses ↑↓ to move the session selection.
- **settings**: category tabs themselves are focusable, Enter activates a category; Down enters the category's
  editable items, Enter opens numeric or choice editing. Item positions within a category are remembered per page
  and auto-scroll when exceeding the visible height.
  ```
                Appearance  Behavior  Display  Advanced
    Theme                ● ferra   ○ custom
    ferra preset or custom palette (custom palette edited by hand in TOML)
    Plain-color mode      ○ on   ● off
    degrade to plain-color output (NO_COLOR semantics)
  hjkl/arrows move  Enter execute  Esc exit · save immediately
  ```
- **Two columns**: left is a smaller name column (30%): name fg, description **Bark** foreground (wraps when too
  long); right is the value.
- **Selection highlight**: only the **name** is highlighted (Night base), the description is not; when editing,
  focus moves to the value and the name is un-highlighted.
- **Value presentation**: unselected option = `○ text` (default fg); selected = `● text` (green). Booleans are
  on/off two options; input-type (numeric) shows text directly, green + cursor block when editing (Night base);
  choice-type editing moves the cursor with `←/→`, with the option under the cursor on a Night base.
- **Edit semantics**: `Enter` confirms, `Esc` cancels back; keys do not leak during editing. After leaving the
  panel the message stream/input bar state restores as-is.
- Default config: `crates/e-tui/assets/default_config.toml` is compiled into the single exe via `include_str!` and
  parsed into `Config::default()` at startup; defaults must not maintain parallel Rust literals. The persisted
  `Config` itself is the only `#[serde(deny_unknown_fields)]` schema, with the runtime resolved theme cached via
  `#[serde(skip)]`. Loading first recursively overlays the user TOML's known keys onto the embedded TOML, then
  strictly deserializes once; old files inherit defaults for missing fields, deprecated unknown keys are filtered,
  and known-key type errors or malformed TOML fall back entirely to the embedded defaults after diagnosis.
- Saving: **save immediately** to `%APPDATA%\dshe\config.toml` and take effect immediately.

**Configurable item list (editable in TUI)**

| Category | Item | Type | Default |
|------|------|------|------|
| Appearance | spinner style (A/B/C/D/E) | enum | A half-moon rotation |
| Appearance | spinner frame rate | numeric ms | 120 |
| Appearance | theme (choose from `%APPDATA%\dshe\themes\*.toml`; palette and semantic mapping edited in two-layer TOML) | enum | deepseek-e |
| Appearance | plain-color mode (NO_COLOR) | boolean | off |
| Behavior | remember last session | boolean | **off** (new process creates a new session by default) |
| Behavior | default mode (preset used by bare `/new` and new-process session creation, from the bridge `presets` roster; a stale config value is still shown/selectable, bridge falls back to standard) | enum | standard |
| Behavior | paste placeholder threshold | numeric chars | 1000 |
| Behavior | long content fold threshold | numeric lines | 20 |
| Behavior | atomic block fold threshold | numeric lines | 40 |
| Behavior | copy hint duration | numeric seconds | 2 |
| Behavior | input history entries | numeric | 1000 |
| Display | tool elapsed display | boolean | on |
| Display | read auto-merge | boolean | on |
| Display | message timestamp | boolean | off |
| Display | mermaid rendering (off = source fence) | boolean | on |
| Advanced | bridge address / port / token path | read-only display | — |

**Not editable in TUI (D29)**: connection parameters (changing them disconnects; startup flag / env var only),
font size (terminal side), clipboard backend (platform-decided), key rebinding (v2), syntax highlighting theme
(await syntect phase two).

### 4.8 Login settings (/login, D33)

- **Entry**: type `/login` (command only, no shortcut); the input bar becomes the login page, same shape as
  /settings (borderless Ash base, 2/3 page height). The login page is a one-level two-choice menu:
  **API key / Proxy**, Enter enters the corresponding sub-page, Esc returns level by level.
- **API key**: the sub-page lists model providers (`ctx.llm.listProviders()`); Enter enters that provider's key
  entry. The destination goes through `ctx.credentials`'s `apiKeyEnv` reference for that provider
  (`providerCredentialRef` reads from settings, defaulting to `<ID>_API_KEY`); `credentials.set/unset` writes and
  takes effect immediately via `credentials/updated`. **The key value is never sent back** — downstream only
  carries `configured/writable/source/hint(…last four)`; the edit box draws ● when typing, and environment-variable
  sources are read-only and do not gain executable focus.
- **Proxy**: lists saved proxies + `+ New`; an existing proxy first enters a "cancel/delete" confirmation page on
  Enter, and `login-proxy-delete` is only sent after explicitly focusing "delete" and pressing Enter. The create
  form fills base url / api key / protocol mode (`openai-completions` / `openai-responses` /
  `anthropic-messages` two-choice) / model name. Destination stored in `%DSH_HOME%\dsh-tui-proxies.json` (api key
  not sent back).
- **Error presentation**: write failures return from the bridge via the same `login` frame's `error` field, shown
  in the panel footer (red ✗), not through the transcript error stream.

### 4.9 Model and theme Input Pages

- `/model` is a single-focus two-column page: providers on the left, the selected provider's models on the right;
  left/right or h/l cross columns, up/down or j/k move within a column, Enter on a provider activates that column,
  Enter on a model sends `model-set`. The green `●` only marks the currently applied model, and the Night
  background marks the current focus — the two must not be confused. During async catalog refresh, focus is kept
  by provider/model id; an empty catalog only shows an explanation and does not create a fake focus.
- `/theme` uses the theme name as the executable focus, with color swatches as decoration only; Enter applies and
  persists the theme. Both pages use the §4.7 common shell, no longer a centered floating window; over-small
  terminals use bounded clipping and produce no out-of-bounds region.

## 5. Bridge and protocol (wire protocol v5)

### 5.1 Endpoint and security

- Endpoint: `ws://127.0.0.1:<dsport>/dsh-tui`; v1 loopback only; auth see O7.

### 5.2 Message protocol (JSON, single-contract generation)

The only machine-readable source for message names, surface events, capacities, `shapeTypes`, payload `records`,
and crates/e-dsh/server `messageShapes` is [`bridge/protocol-contract.json`](../bridge/protocol-contract.json).
`node tools/sync-protocol-contract.mjs` validates that JSON and sync-generates [`docs/protocol.md`](protocol.md),
Rust `build.rs` constants/shape JSON, Rust/Node conformance fixtures, and
`bridge/package.json.dshCompatibility.wireProtocol`; `--check` fails on any unsynced derivative.
`tools/generate-protocol-doc.mjs` is just the sync tool's compatibility entry point. Both the Node bridge runtime
and the Rust build read this single contract; never hand-write snapshot/history/frame numbers or an independent
message roster on either side.

`hello` carries `protocolVersion` and may carry `resumeSessionId`, `cwd` (the TUI launch directory), and `mode`
(only the preset id when creating a startup session). `welcome` replies with `protocolVersion`, `maxFrameBytes`,
session id/status, and optional `title`, `cwd`, provider/model; older peers may omit new capability fields. The
normal frame cap is 16 MiB; only when connecting to an un-upgraded old bridge can you explicitly set
`DSHE_LEGACY_MAX_FRAME_MB` to relax the client cap.

After each attach the bridge also sends `commands{commands:[{name,description,input?:{hint}}]}` from
`ctx.commands.list(agent)` (no handler); the client merges with the built-in optimized commands, with built-ins
overriding same names. `commands/change` triggers a per-connection agent-scoped full refresh. Generic commands
still execute via `command{line}`, with direct UI results returned as
`command-result{commandId,kind:success|error,text?}`; followup-type commands continue to be presented through
ordinary session events; when `execute` returns `undefined` the bridge sends `command-unknown` and does not create
a model message. The existing `interrupt` frame also aborts every in-flight command execution controller; an
aborted direct command settles client-side through the silent `command-cancelled` error acknowledgment.

`login` payload `{ providers: [{id,name,apiKeyConfigured,apiKeyWritable,apiKeySource?,apiKeyHint?}],
proxies: [{id,name,baseUrl,protocol,model}], error? }` (§4.8): API key is a view only, no value.

`presets` payload `{ presets: [{ id, name?, description?, order?, broken? }] }`: an agent-presets roster snapshot,
sent right after `welcome` on every attach (hello/`/new`/Resume); the client uses it to render the `/new ` mode
prompt popup.

`skills` payload `{ skills: [{ name, description }] }`: the bridge calls `ctx.skills.list` by the attached
session's cwd/scope and sends only the winning `invocation.userInvocable` items; full refresh after each attach and
`skills/change`. The client filters by name prefix→substring→subsequence when typing `/skill`, `/skill:`, or the
compatible space form, and always fills the canonical `/skill:<name>`.

`snapshot` payload `{ events, truncated? }`: contains the rebuild events listed in the contract, plus unknown
events carrying `surfaceOp` (for compatibility degradation of new host events); unknown events are trimmed on the
bridge side to an empty data envelope with only bounded type/seq/time/surface metadata. assistant/chunk does not
enter the snapshot (assistant/message carries the final text). The bridge sends at most the most recent **600**
events, setting `truncated: true` beyond that; history pages via `history{beforeSeq,limit}`, at most 2000 per
page. All downstream JSON enforces the 16 MiB cap by UTF-8 bytes via `frame.js::encodeBoundedFrame`:
snapshot/history keep the newest fitting suffix, and a single over-limit frame becomes `frame-too-large`. After a
cold-log read completes, the original connection's validity must be verified — never send an old session's
snapshot to a rebound socket.

Event JSON converts to typed `HostEventKind` at the `protocol.rs::HostEvent` boundary, also reading the event
top-level `time`, `surfaceOp`, and `sourceEventSeqs`; unknown events stay as `Unknown`, but only unknown
append-surface events generate a bounded fallback — unknown/malformed replace reports a compatibility error and
must not be faked as an append. `projection.rs` exhaustively classifies events into display, surface mutation,
page/session state, input accessory, or ignore effect; the `AppState` reducer no longer walks raw host JSON.

Events display uniformly as four public surfaces:

| Surface | Use | Representative events |
|---|---|---|
| `ActivityRow` | Waiting/Running/Success/Failure/Cancelled activity, optionally with parent/depth | Thinking, tool, retry, command, Code Mode, workflow, compaction |
| `TranscriptBlock` | plain/Markdown/fallback content without work state; in compact mode reasoning blocks fold into the `• Thinking...` breathing light — not rendered, not in copy provenance, and transparent to activity-row adjacency; lines/full mode renders the content directly (lines truncates by post-wrap display row count), and the adjacent Thinking indicator row is then taken over and hidden | assistant, turn notice/error |
| `ContentCard` | content card with uniform padding, background, and copy source; context injection events render as plain text (no shell) with a `提示词注入` label in the activity label tone and the content in the activity detail tone, showing at most 2 lines by post-wrap display row count with a trailing `…` when overflowing, but copy source keeps the full original text | user message, context, attachment placeholder, compaction summary |
| `InputAccessory` | above the input bar, unified height budget/priority/focus | queue, approval, todo, goal, plan |

File activity keeps operation labels via `FileAction`: consecutive `read`, `view`, `edit`, `replace`, `insert` can
fold into the same activity row, where the latter three come from `str_replace_editor`'s view/str_replace/insert
commands; its absolute path is preferentially shown as a workspace-relative path by the session's `session_cwd`.
create does not participate in folding and is shown separately as `<indicator> create <relative-path>`, without
appending tool output line count or elapsed time after completion. Generic tool rows expose their output line count
and elapsed time from the running state onward; single-row truncation shortens the command/summary first and keeps
those trailing metrics visible.

DSH surface replace runs before display: the shadowed surface node and its owned tool activities are deleted from
the effective transcript, and the replacement is inserted back at the original surface position; compaction's
log-only summary only updates lifecycle, and the single visible summary card is created and owned by the
immediately following replacement. When history pages from newest to oldest, the shadowed seq is kept long-term,
so a later-loaded older page does not revive compacted messages; when a tool/command/Code Mode/workflow terminal
half is split from its start by a page boundary, the projector stages the terminal outcome and rebuilds the final
state directly when the older page's start arrives; workflow cancelled is kept as an independent state. When a
retry schedule replays later than an already-loaded retry-started, it writes deferred enrichment and backfills the
full delay/failure/maxRetries and start time after saved rows are restored and the index shift completes.
`session/title`, provider/model, request context, and policy state only update page/session state; `request/header`,
`session/end-seed`, approval audit, and title/search requests are ignored by default.

Markdown/source mapping is still done client-side locally; `TranscriptRenderCache`, `ReadingDocument`,
`ReadingLayout`, and `ProvenanceLayoutRow` share the same width/generation display-row layout rather than deriving
padding/wrap/spacing on each keypress. Wheel, paging, follow, history prepend anchors, Reading overlays, and Item
fragments all use post-wrap display-row coordinates; row count is built by a
linear Unicode grapheme-width scan prefix, combining marks and emoji ZWJ keep the same grapheme across style spans,
and only visible rows are materialized per frame. Streaming text deltas only set `tail_dirty`; after splicing the
tail only the tail row-count suffix is replaced and the prefix is written incrementally; pure breathing/settle
animations only patch the active message's stable line range, and settle expiry first clears the interpolation
source marker (keeping the completion-time sentinel), submits the precise target-color patch, then stops the
deadline. A range line-count change safely falls back to full rebuild; structural changes like surface replace only
invalidate once.

The main loop uses Crossterm `EventStream` to feed keyboard/mouse/paste/resize directly into `tokio::select!`, no
longer relying on 50ms input polling; interaction frames 16ms, content frames ~30ms, animations coalesced by the
`spinner_frame_ms` deadline, with zero-cycle wakeup when idle. Bridge inbound bursts process at most 64 messages
or ~2ms per turn to avoid starving input/expiry frames. `TerminalOwner` manages raw mode, alternate screen, and
restore at a single point, and the CrosstermBackend uses a 64KiB BufWriter; each frame wraps the diff, hidden IME
anchor, and flush with DEC private mode 2026 Begin/End synchronized output — terminals that don't support the
extension ignore the sequence and continue with normal diff output, and `DSHE_DISABLE_SYNC_OUTPUT=1` explicitly
disables it for diagnosis.

## 6. Rust tech stack

| Layer | Choice | Notes |
|----|------|------|
| TUI | ratatui + crossterm EventStream | double-buffer diff, event-driven input, buffered/synced frames, resize, mouse |
| Async/WS | tokio + tokio-tungstenite | connect to the bridge endpoint |
| Protocol | serde + serde_json | strictly aligned with §5 |
| Markdown | pulldown-cmark | preserve block ranges for the source map |
| Mermaid | **wasmi + grok-mermaid WASM** | in-process interpretation; degrade to source fence on failure |
| Code highlight | plain color + language label | syntect deferred |
| Clipboard | arboard (system clipboard) | Windows writes the clipboard directly |
| Config | toml + serde (embedded `crates/e-tui/assets/default_config.toml` + `%APPDATA%\dshe\config.toml` overlay) | missing fields inherit defaults, save immediately (§4.7) |
| Wide chars | unicode-width | CJK/emoji width |
| Distribution | single exe (repo root is a Cargo workspace, root `cargo run` launches) | client runtime has no Node dependency; first install/update of the bridge needs Node.js/DSH |

### 6.1 Source install path

The README "Quick Start" is the current user-facing install entry: on Windows / PowerShell, first prepare Git,
Node.js/npm, and Rust/Cargo, install `@deepseek-ai/dsh` globally, use `cargo install --path crates/e-dsh --locked` to
install `dshe.exe` into the Cargo bin directory, then run `dshe setup`. The bridge runtime (package manifest,
canonical protocol contract, and every production `bridge/src/*.js` module) is embedded in `dshe.exe` at build
time; `dshe setup` materializes it into the dedicated `dshe` profile, registers it idempotently, runs the
equivalent of `dsh plugin --profile dshe install`, validates the result, and atomically records the successful
bridge digest in `%DSH_HOME%\profiles\dshe\.dshe-setup.json`. When `DSH_HOME` is unset, empty, or whitespace-only,
setup and the client both use `%USERPROFILE%\.dsh`, and the resolved value is passed to the DSH plugin child
process. Setup only merges the bridge-owned registrations (`dependencies["dsh-tui-bridge"] = "workspace:*"`,
the `packages/*` workspace entry, and the `tui-bridge` patch insert) and preserves unrelated profile
configuration; malformed profile files are refused with an actionable error rather than rewritten. The
`tools/mount-bridge.ps1` script remains a development-only shortcut for hot-syncing `bridge/` (and the `web`
profile) without a client rebuild, but it is no longer a user prerequisite.

Normal TUI startup is gated on the setup record: before acquiring DSH, reading the token, opening WebSocket, or
initializing the terminal, the client classifies setup as ready, missing, outdated, or damaged from the record's
embedded bridge digest plus cheap structural checks, and refuses to proceed unless ready. A client-only update
whose embedded bridge is unchanged remains ready; any bridge change (or a legacy script-mounted profile without a
record) requires a one-time `dshe setup`. All setup and startup-gate failures are English and actionable, naming
the failed condition and the exact next command or repair step. After install the client runtime remains a single
exe; Node.js is only used for DSH itself and for installing the bridge's npm dependencies. After changing the
bridge or upgrading DSH, rebuild/`dshe setup`/restart and then run `npm run verify-dsh-upgrade` from `bridge/`;
it checks the generated contract, declared DSH/agent versions, and public exports, and runs helper and
session-routing smoke against the deployed copy.

## 7. Platform and boundaries

- Windows is the primary target: crossterm natively supports Windows Terminal / ConPTY; width via unicode-width.
- Not doing: inline images in the terminal (no iTerm2/kitty protocol), attachments shown as a reference line.
- Not doing (v1): horizontal scroll, file-path completion, multi-column layout, key rebinding, partial selection
  within a block (tables/mermaid/code).
- Theme selection is already in the settings panel (§4.7); the open palette and fixed semantic mapping are
  currently edited via theme TOML.

## 8. Decision wrap-up

All open questions (O1–O16) have been discussed item by item and recorded as D-series decisions. The remaining
items are **implementation verification items**, not design questions:

- grok-mermaid WASM is integrated with wasmi, with success/failure degradation tests.
- The bridge token is fixed at `%DSH_HOME%\dsh-tui.token`, read by the client at startup.
- The ferra-to-256-color degradation map — generated at implementation time with an algorithm (nearest color
  distance).
- 2026-08-18 Brooks Architecture Audit: 94/100; production dependency graph SCC-free, single-track transcript/
  strict Config/canonical contract all have automatic guards. Full graph and remaining `AppState` cognitive-load
  suggestions in [`architecture-audit.md`](architecture-audit.md).

## 9. Milestone draft (refined after design finalization)

1. M1 bridge plugin + protocol integration (TS side + minimal Rust client echo)
2. M2 message stream rendering + input + streaming + interrupt
3. M3 markdown + table rendering (including line-map source map skeleton)
4. M4 semantic Reading View, responsive Preview, and atomic whole-Block copy
5. M5 mermaid (WASM) + approval card + session selector + help overlay
6. M6 completion/history/multi-line + settings panel (config.toml + /settings overlay) + ferra theme polish +
   packaging/distribution
