# Execution tasks

- [x] 1.1 Implement provider-neutral session identities, creation reservations, focus/swap/close ownership and strict command parsing.
  - Design: [workspace API](implement.md#apis-and-data-model).
  - Acceptance: Compile the shared API; no provider payloads or I/O, no reused identities, stale attachment returns the candidate for cleanup, swap preserves focus, close promotes the survivor.
- [x] 1.2 Encapsulate the Pi process/adapter pair and use that owner in the existing runner.
  - Acceptance: Existing Pi startup and shutdown retain behavior; each owner has independent transport and adapter state; compile Pi and run existing adapter checks.
- [x] 2.1 Implement native independent new/clone/fork preparation with safe checkpoints and cancellable fixed fork choices.
  - Dependencies: 1.1, 1.2
  - Acceptance: Starting a candidate never mutates or pauses the source; no shared writable native session file; cancellation/failure does not attach a pane or send a prompt; isolated native checks confirm persistence and snapshot behavior.
- [x] 2.2 Multiplex independently owned Pi runtimes and all session-local effects using stable frontend identities.
  - Dependencies: 1.1, 1.2, 2.1
  - Acceptance: Both queues and streams progress while unfocused; side startup, snapshot/choice loading and materialization never block page input, scrolling, focus or rendering; local failures and stale work do not cross sessions; shutdown cleans up both runtimes and their child effects. Retain the startup owner and select on completion rather than awaiting startup inline.
- [x] 3.1 Integrate two conversation panes, shared Preview, input geometry, mouse focus/scroll and atomic selection presentation.
  - Dependencies: 1.1, 2.2
  - Acceptance: Equal conversation widths use the current Preview layout; independent drafts and scroll survive focus/swap; narrow resize preserves runtimes. Preview entry/exit behavior follows task 3.3.
- [x] 3.2 Wire command discovery, completion, busy-close confirmation, exit interception and Help.
  - Dependencies: 2.1, 2.2, 3.1
  - Acceptance: All proposal commands work; no <code>/side main</code>; duplicate creation rejects locally; DSH does not claim unsupported capabilities; exit closes only the focused conversation when split.
- [x] 3.3 Automatically collapse Preview on successful side attachment and restore its prior layout on exit.
  - Acceptance: Preserve prior width/collapsed and fullscreen state; restore after closing either role, including swap/promotion; do not change layout for pending/failed/cancelled creation or declined close. Temporary widths must not leak through config persistence. Admission accounts for the collapsed width.
- [x] 3.4 Show side-opening progress in the composer bottom rule.
  - Acceptance: Bare creation and new/clone/fork preparation/startup use the localized opening label and existing tool spinner cadence in Honey, right-aligned without adding rows. Tips pause while displaced. Success, failure, cancellation and stale reservations clear it; focus-only operations do not show it. Input remains responsive. No test edits.
- [x] 4.1 Synchronize delivered current contracts and run overall verification.
  - Dependencies: 3.1, 3.2
  - Acceptance: Relevant existing tests, workspace formatting/Clippy and isolated manual Pi/TUI checks pass or have explicit limitations; current documentation describes delivered behavior; no test edits or README edits; change remains active.

## Verification

### Build and existing checks

- `cargo check -p e-pi -p e-tui -p e-dsh` and `cargo build -p e-pi --bin pie`: passed.
- `cargo fmt --all --check`, `git diff --check` and `node --check` for the auth companion, side companion and side helper: passed.
- `cargo clippy --workspace --all-targets`: completed successfully. Existing e-tui/DSH warnings remain; no warnings were reported for the new Pi worker, workspace or artifact code.
- Existing Pi checks passed: binary tests (3), `adapter::` (54), `auth::` (6), `process::` (3), and the architecture integration target (2).
- Existing e-tui checks passed: `command_catalog::` (3), `runtime::controller::` (71), `ui::` (130), and `selection::` (12). These scoped runs overlap; counts are not a distinct-test total.
- `doco check side-sessions`: mechanical checks passed. Current architecture, runtime, interaction and presentation contracts now describe the delivered behavior.

### Initial delivery: isolated native and real-terminal exercises

The initial delivery preserved Preview allocation. Task 3.3 supersedes that entry/exit layout policy; the observations below describe the initial revision.

Temporary native sessions and an offline provider were driven through real Pi JSONL processes and Windows `tui-test` PTYs. No paid model requests or real user sessions were used.

- Native snapshot checks confirmed distinct identities and ancestry, unchanged source-file SHA-256, exclusion of incomplete history, rejection of unresolved tool calls and stale identities, and explicit non-persisted empty-session results.
- Two separately recorded Pi process IDs streamed concurrently. Opening and cancelling fork choices left source streaming active. Bare <code>/side</code> cloned a safe completed checkpoint while the source continued its current turn; swap did not stop either stream.
- Both conversations accepted queued work. Each unfocused queue dispatched exactly once to its original process, and all four offline turns completed.
- New omitted source history; fork restored the selected user text without submitting it. Fork cancellation, invalid/trailing arguments, duplicate creation, and narrow-window admission were checked. Side-only operations made no provider requests and left source history unchanged.
- Independent drafts survived focus and swap. Wheel input over the other pane preserved keyboard focus. Narrow resize retained Preview and showed one focused pane; widening restored both. Preview followed focus and restored the original reading selection on return.
- Busy close was declined without changing the split, then ordinary exit was confirmed to close only the selected process. The other process remained alive and was promoted. Closing the original main also promoted side and retained saved history.
- A delayed launcher verified typing and resize during pending startup, duplicate rejection, cancellation, candidate-file deletion, no late attachment, and successful retry. A startup timeout preserved source draft/layout, killed the candidate and permitted retry.
- Repeated cancellation at different preparation timings removed staged/native candidate files, preserved source SHA-256 and permitted a subsequent clone. Staging is private to companion ownership; uncommitted file owners cover event-channel drops and failed attachment.
- Killing one side process left the sibling stream advancing and the workspace interactive; the failed pane could be closed independently. Failed transcript activity is settled rather than left spinning.
- Normal exit checks confirmed that recorded native processes and companion/staging directories were gone. The delayed Windows launcher exercise also verified descendant cleanup. Probe fixtures and their owned history/state entries were cleaned or restored.

### Preview layout follow-up

- `cargo check -p e-pi -p e-tui -p e-dsh`, `cargo build -p e-pi --bin pie`, `cargo fmt --all --check` and `git diff --check`: passed.
- Existing `e-tui` checks passed: `ui::screen::` (4) and `runtime::controller::` (71). No tests were added or modified.
- An isolated real-Pi Windows PTY exercise confirmed automatic collapse on clone/new attachment, restoration after swap and main promotion, original collapsed/fullscreen restoration, and admission using the collapsed width at 70 columns. Fork cancellation and failed startup left the original Preview unchanged.
- The same exercise reopened Preview by dragging its collapsed grip, saved an unrelated frontend setting, and checked the persisted width: neither automatic collapse nor temporary resizing overwrote the original preference. Declining a busy shell conversation's close retained its temporary layout; confirming close restored the original layout. Temporary sessions, fixtures and owned frontend state were cleaned or restored.

### Opening-indicator follow-up

- `cargo check -p e-pi -p e-tui -p e-dsh` and `cargo build -p e-pi --bin pie`: passed. Scoped rustfmt checks on changed Rust files and `git diff --check`: passed.
- Existing `e-tui` checks passed: `ui::` (130), `runtime::state::animation::` (1), and `i18n::` (4). The `tips` filter matched no existing tests; no tests were added or modified.
- An isolated offline Pi Windows PTY exercise with delayed candidate startup confirmed the English bottom-rule label in Honey (`#f5d76e`), advancing tool-spinner frames, editable drafts during loading, immediate removal on Esc, removal after failed startup, and successful bare <code>/side</code> attachment without a lingering label. Focus-only <code>/side</code> showed no loading label. At 30 columns, the shell label retained its space and the opening text clipped while its spinner remained visible.
- Tips displacement/resumption and fork preparation use the shared reservation/render paths; they were not separately exercised during live agent work. Chinese localization was checked by the existing locale checks, not a Chinese-language PTY run. No paid model requests were made. Temporary native sessions and owned frontend state/history were cleaned or restored.

### Limits and phase boundary

Live terminal validation was performed on Windows, not across every supported terminal/platform. Real OAuth, network retries and external MCP services were not exercised end to end; existing checks and per-conversation ownership/routing provide the coverage for those paths. Selection checks combine existing tests and source verification of the shared-frame commit and region-scoped replay, rather than a new split-pane clipboard test.

No repository tests or README files were added or modified. Implementation and validation are delivered; the change remains active and has not been completed or archived.
