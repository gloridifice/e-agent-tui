# e

**e** (also called **e tui**) is a terminal UI client for DeepSeek Harness (DSH). Its executable is **`dshe`**, a single binary with no extra runtime dependencies.

It connects to the same DSH backend as the Web GUI and shares the same session logs, so you can switch between the terminal and the browser at any time.

## Highlights

- **Streaming chat** — live output, thinking lines, spinners, and full Markdown rendering (headings, tables, code blocks, mermaid)
- **Tool cards** — command summaries with line counts and timing; nested Code Mode and workflow work is shown as parented activity rows
- **Event-aware transcript** — reasoning, context/attachment cards, retries, durable commands, compaction, rich turn outcomes, and DSH surface replacement are projected consistently
- **Input accessories** — queued prompts, approvals, questions, todos, goals, and plan mode share a bounded area above the editor
- **Unified Input Pages** — `/settings`, `/login`, `/model`, and `/theme` replace the editor with one borderless, keyboard-navigable page instead of opening floating windows
- **Model selection** — switch provider and model with `/model`
- **Themes** — `deepseek-e` (default) and `ferra` built in; custom themes supported
- **Sessions** — create, switch, and resume conversations with incremental history
- **Unified commands** — optimized built-ins and commands contributed by DSH/plugins share one fuzzy-completion menu; plugin changes appear live
- **Responsive input** — queue prompts while the agent runs; queued dispatch and copy-mode navigation do not block the UI
- **Fast** — incremental rendering cache, throttled redraws, only the visible window is drawn

## Installation

Requires [Git](https://git-scm.com/), [Node.js](https://nodejs.org/) (with npm), and [Rust](https://rustup.rs/) (with Cargo). Windows / PowerShell is the primary platform.

```powershell
# 1. Install DeepSeek Harness
npm install --global @deepseek-ai/dsh

# 2. Clone this repository
git clone https://github.com/gloridifice/e.git
cd e

# 3. Install the TUI bridge (an unset/empty DSH_HOME automatically uses $HOME\.dsh)
# Optional custom home: $env:DSH_HOME = 'D:\path\to\.dsh'
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install

# 4. Build and install the dshe binary
cargo install --path client --locked

# 5. Start it from the directory you want to work in
dshe
```

On first launch, `dshe` starts the DSH service automatically, then guides you through signing in (API key / account / proxy); use `/model` to pick a model. If PowerShell cannot find `dshe`, add `%USERPROFILE%\.cargo\bin` to your `PATH`.

> **Updating**: re-run step 4 after pulling new code. If the bridge changed, repeat step 3 and restart DSH.

## Commands

Type `/` to open command completion; continue typing for prefix/substring/fuzzy matching, then use `↑`/`↓` or `Tab` and `Enter`.

Commands use two compatibility levels:

- **Built-in commands** are optimized for dshe (for example `/settings`, `/model`, `/new`, and `/resume`). Their effect, description, input hint, and specialized completion policy are declared together in `client/src/runtime_command.rs`; `/new ` completes the live agent-preset roster.
- **Integrated commands** come from DSH core or any installed DSH plugin. The bridge discovers the effective per-session `ctx.commands` registry automatically, refreshes it on `commands/change`, and forwards execution results directly to the transcript. No dshe code change is needed when a plugin registers a new command.

DSH 0.1.0-rc.6 exposes command names, descriptions, and one free-form input hint, but no typed argument-completion schema. Therefore every integrated command has name completion and shows its input hint; richer argument completion is available only for built-ins that dshe explicitly optimizes.

## Keyboard quick reference

| Keys | Action |
|---|---|
| `Enter` | Send the editor contents |
| `Shift+Enter` | Insert a newline |
| `↑` / `↓` | Move between editor lines; at the first/last line recall the previous/next prompt |
| `PageUp` / `PageDown` | Scroll the message transcript by one visible transcript page |
| mouse wheel | Scroll the message transcript by three rows |
| `Ctrl+H` | Show help (`^h Help` in the status line) |
| `Ctrl+B` | Enter transcript copy mode |
| `Ctrl+N` | Open the session picker |
| `Esc` | Cancel/return in an Input Page or interrupt active model work |
| arrows or `hjkl` | Move the single focus between actionable Input Page elements |
| `Enter` in an Input Page | Execute the focused element |

`/settings`, `/login`, `/model`, and `/theme` use the shared **Input Page** layout: no border or floating window, one row of vertical padding, two columns of horizontal padding, and one stable focus. Text editors consume ordinary letters—including `hjkl`—instead of navigating. Existing proxies open an explicit confirmation page before deletion.

The two background-free status rows are:

```text
<work indicator> <mode> <model> CH<cache-hit%>                         ^h Help
<session title, or 新会话>                                   <absolute workspace path>
```

## Architecture and protocol

`client/` is the Rust TUI and `bridge/` is the DSH host plugin. Their JSON WebSocket contract has one machine-readable source: [`bridge/protocol-contract.json`](bridge/protocol-contract.json). [`docs/protocol.md`](docs/protocol.md) is generated with:

```powershell
node tools/generate-protocol-doc.mjs
```

The normal WebSocket frame limit is 16 MiB. The bridge enforces it on every outgoing frame; oversized snapshot/history frames retain the newest fitting suffix and singular oversized frames become a bounded compatibility error. Only when connecting to an old, not-yet-restarted bridge, set `DSHE_LEGACY_MAX_FRAME_MB` explicitly (for example `64`) before launching `dshe`.

DSH events are translated into typed `HostEvent` values at the wire boundary and classified by the event projector. User-visible output uses four shared surfaces: status-bearing activity rows, ordinary transcript blocks, padded content cards, and input accessories. DSH `surfaceOp` append/replace metadata is applied before rendering, including across backward history paging, so compaction does not leave shadowed messages visible. Unknown append-surface events also survive reconnect/history replay through bounded metadata-only envelopes, lifecycle pairs split across page boundaries reconcile when their older start arrives, retry schedule details are merged back into newer started rows, and workflow cancellation stays distinct from failure. Title/session state and audit-only records remain outside the transcript.

The transcript renderer owns its cache, and copy-mode navigation consumes the same layout provenance as the visible transcript, preventing spacing and wrapping drift. Streaming text still dirties only the tail; structural replacements invalidate the transcript once.

## Development

```powershell
cargo test
cd bridge; npm test
node tools/generate-protocol-doc.mjs
```

Bridge lifecycle policy is split into testable host, connection, history, session, command, dispatcher, and protocol adapters under `bridge/src/`. Slash completion merges the effective per-session DSH/plugin command catalog with client-optimized commands; live DSH compatibility remains covered by `node tools/smoke-bridge.mjs`.
