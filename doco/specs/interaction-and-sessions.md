# Interaction and session contracts

## Input ownership

- Input, page, Reading, approval, search/completion, and link-selection contexts MUST resolve semantic actions before text insertion. Disabled child actions MUST NOT fall through to parent behavior.
- Composer cursor positions are character indices; byte slicing MUST use explicit character-to-byte conversion. Atomic paste/image blocks MUST be skipped or removed as units and MUST expand losslessly on submission.
- Composer Up/Down MUST follow the rendered wrapped rows at the current content width, preserving the display-cell column where possible, regardless of multiline mode. Navigation MUST respect grapheme and atomic-block boundaries; prompt history is reached only beyond the first or last editable visual row. Search and completion retain their own navigation.
- Input Pages MUST use one closed frontend-owned session/controller and return effects for execution after state borrows are released. Text-edit states treat ordinary letters as text.
- `@` path completion MUST match the unfinished name within the selected directory using a case-insensitive contiguous substring; it MUST preserve native parent-directory resolution, exact-directory descent, candidate spelling, and directory-first ordering. It MUST NOT perform recursive or subsequence matching.
- Help is a blocking input context. It owns close and inherited full-screen scrolling actions while visible; reopening resets its independent scroll position without changing the composer or underlying page.
- Mouse wheel routes to the pane under the pointer. While Help is visible, the wheel scrolls the modal; otherwise separator capture takes priority over text selection.

## Authentication pages

- Authentication pages discover provider IDs, labels, methods, safe status, and removability from the adapter. They MUST support correlated text, secret, choice, authorization-link, device-code, progress, cancellation, withdrawal, and terminal outcome events without hard-coded provider sequences.
- Secret and manual callback editors are ephemeral, start empty, mask display, and drop their buffers on submit, cancellation, or page replacement. Authentication mutations are admitted only while the conversation is idle and MUST NOT materialize a draft session or enter ASAP/follow-up queues. Leaving a page with an active flow cancels that flow; every native page MUST remain closable even when a result never arrives.
- Pi `/login [provider]` resolves a stable ID first and otherwise an unambiguous name. Pi `/logout` lists only stored credentials that native logout can remove and requires confirmation. Unknown references and unavailable native support remain local errors and MUST NOT become model prompts. DSH retains its API-key/proxy login workflow.

## Prompt queues

- Pending prompts preserve FIFO within ASAP and after-turn classes; ASAP candidates have dispatch/display priority.
- Admission and clear operations MUST be serialized per session. Their results MUST carry authoritative queue state and session identity; stale-session results MUST be ignored.
- Cancel removes ASAP candidates before after-turn candidates. A clear barrier prevents repeated cancellation and holds newer submissions until acknowledgment.
- Failed admission remains visible without automatic retry. Failed clear preserves the backend snapshot and reports an error.

## Session lifecycle

- Session replacement MUST clear session-scoped questions, approvals, queues, catalogs, and stale async ownership together.
- A bare interactive `/new` creates a frontend draft. Its first prompt or explicit skill atomically materializes the new session; old-session updates continue reducing but remain hidden from the draft.
- New-session failure restores the draft input. Model/effort selection made during the draft applies to the materialized session.
- Resume results MUST preserve stable identity and selection through progressive updates. Optional parent relationships MUST be admitted with the same page/workspace generation as their session batch. Native session reads remain adapter-owned, bounded, read-only, and outside UI locks.
- Pi `fork` and `clone` slash commands MUST use native session replacement, not rewrite session files or send command text as a model prompt. Fork selects a historical user entry and branches before it; clone duplicates the current active branch. Without arguments, fork restores the selected text to the composer and clone leaves it empty. An optional trailing message MUST be sent only after successful replacement and authoritative state/history refresh; cancellation or failure MUST NOT dispatch it. Replacement is admitted only while idle and MUST exclude concurrent adapter mutations until refresh and any trailing prompt admission settle.
- Pi CLI resume by exact session ID MUST resolve an existing native session and its saved workspace before terminal setup, without opening a picker, creating a replacement, or forking. Missing or ambiguous IDs and conflicting session selectors MUST fail explicitly. Discovery honors the adapter's native session-directory overrides.
- On TUI exit, `pie` MUST print a recovery command for the last attached saved session after terminal restoration. Unsaved sessions MUST NOT produce recovery commands; files outside native session discovery use an explicit file command instead of an undiscoverable ID.

## Execution history

- Opening history starts in the longest-operation ranking. The history-scoped `toggle_view` action defaults to Tab and switches between ranking and chronological timeline without issuing another query or changing the composer draft.
- Ranking and timeline keep independent vertical offsets. Existing full-screen movement, mouse-wheel, and exit actions apply to the active view; all bindings remain semantic and configurable.

## Commands and model selection

- Built-in commands have one metadata registry. Adapter command catalogs MAY extend it, but built-ins win name collisions.
- Direct commands return typed command results and MUST NOT become model messages.
- `/model <model-id> set-default-effort <effort>` saves an e-only preference without changing the current session route. It accepts canonical or unique bare model ids, validates and completes efforts against the target model, and rejects invalid or ambiguous input locally. Subsequent direct/menu model selections and marked prompts use a supported stored default; explicit effort changes, resumed sessions, and restored temporary routes are not overridden. Queued marked prompts capture their default at admission. Unsupported saved defaults are retained but not applied.
- Model and effort changes are confirmed by the adapter before dependent prompt admission. Temporary marked prompts strip only the mark, preserve exact queued route, and restore the original model/effort after authoritative idle.
- Compaction routing remains adapter-owned; its user-wide persistence is defined by the configuration contract. Pi manual compaction with an override MAY admit serialized model reads and selections only after the native run captures its compaction model; the original conversation route is the fallback, and each newer authoritatively confirmed conversation selection supersedes it. Prompts, session/resource mutations, and finalization-time model controls MUST remain held until compaction settles and the effective return route is restored and verified. Any other temporary model selection MUST restore and verify its prior route before releasing dependent requests.
- `/reload` reloads frontend settings and backend resources, not just the displayed directories. Pi uses its native resource reload before refreshing command/skill/model catalogs; DSH invalidates backend skill discovery before refreshing its scoped skill/command/model catalogs. DSH plugin replacement remains a restart operation. Backend errors MUST remain visible and MUST NOT be reported as successful reloads; stale results MUST NOT update replacement sessions.
