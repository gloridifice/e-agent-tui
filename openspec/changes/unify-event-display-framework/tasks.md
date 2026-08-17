## 1. Baseline and Fixtures

- [x] 1.1 Add bounded Rust fixtures for core DSH 0.1.0-rc.6 events, content blocks, top-level timestamps, source forms, tool errors, and append/replace surface metadata.
- [x] 1.2 Add bridge fixtures for live and cold logs containing commands, compaction, retries, nested Code Mode calls, workflows, todos, and audit-only records.
- [x] 1.3 Add UI equivalence tests that lock the current user card, assistant Markdown, Thinking, generic tool, file-group, lifecycle notice, queued-message, approval, and question presentation before migration.

## 2. Shared Display Models

- [x] 2.1 Create focused client display modules defining stable display identity, `ActivityRow`, `TranscriptBlock`, `ContentCard`, lifecycle/tone/content types, and composition metadata.
- [x] 2.2 Implement the shared ordinary-block and padded-card layout paths with width-aware wrapping, background clearing/fill, stable source units, and copy provenance.
- [x] 2.3 Implement the shared activity-row layout with lifecycle colors, timing, parent/depth indentation, animation state, and activity-row composition spacing.
- [x] 2.4 Migrate user messages, assistant Markdown/streaming, and lifecycle notices to card/block models while preserving tail-splice and Markdown materialization behavior.
- [x] 2.5 Migrate Thinking, generic tools, and read/edit file groups to the activity contract while preserving grouping, animation, duration-toggle, and interruption behavior.

## 3. Shared Input Accessories

- [x] 3.1 Define the `InputAccessory` model, deterministic priority/focus rules, height budget, informational collapse behavior, and one shared bottom-layout calculation.
- [x] 3.2 Migrate queued prompts, approvals, and questions to the accessory layout and key-dispatch path without changing their visible interaction.
- [x] 3.3 Add small-terminal, coexistence, focus, opacity, and hard-coded bottom-row regression tests for the accessory stack.

## 4. Typed Event Projection and Surface Semantics

- [x] 4.1 Extend `HostEvent` parsing with top-level `time`, `surfaceOp`, `sourceEventSeqs`, structured content blocks, context source metadata, complete turn reasons, and tool-result block errors.
- [x] 4.2 Introduce typed `ProjectionEffect` values and an event projector with stable correlation indexes for surface nodes, tools, commands, retries, compactions, nested calls, and workflows.
- [x] 4.3 Implement append-surface placement and display ownership without allowing `serde_json::Value` into the projector or application reducer.
- [x] 4.4 Implement replace-surface mutation, including repeated replacement, paired tool-row removal, stable insertion position, and one structural cache invalidation.
- [x] 4.5 Make backward history paging retain shadowed sequence metadata, suppress later-loaded shadowed nodes, and preserve the viewport by effective rendered row count.
- [x] 4.6 Add bounded fallback presentation for unknown append-surface events and a non-destructive compatibility error for unsupported or malformed replacements.

## 5. Core Content and Outcome Coverage

- [x] 5.1 Accumulate and render assistant text/reasoning chunks by turn and step, finalize them from `assistant/message`, and avoid duplicate final content.
- [x] 5.2 Project direct user messages, context forms, notices, and image attachment placeholders into the appropriate card/block surfaces.
- [x] 5.3 Project completed, aborted, blocked, interrupted, error, and max-token turn outcomes with bounded structured details and no empty step rows.
- [x] 5.4 Correct tool failure/timing semantics and qualify or omit line counts when the bridge supplied only a trimmed result tail.
- [x] 5.5 Add reducer and UI tests for reasoning, context forms, attachments, every core turn outcome, tool `isError`, top-level timing, and interrupted streaming/tool settlement.

## 6. Extended Event Families

- [x] 6.1 Project `todo/write` as whole-list accessory state, retire it on the next turn, and test replacement rather than append semantics.
- [x] 6.2 Project `llm/retry` and `llm/retry-started` into one correlated retry activity per producer/step with waiting, running, and terminal states.
- [x] 6.3 Project durable `command/run`/`command/done` activities and deduplicate their live direct-result frames so reconstructed and live transcripts agree.
- [x] 6.4 Project `tool/code-dispatch-start`/`tool/code-dispatch` as parented child activities under the root tool call.
- [x] 6.5 Project workflow run/member start/end events into stable hierarchical activities with bounded labels and outcomes.
- [x] 6.6 Project compaction lifecycle presentation independently from mandatory replacement semantics, including operation without a specialized compaction card.
- [x] 6.7 Classify goal/plan state as informational accessories and title, model, request-context, usage, preset, permission, sandbox, and schedule state as non-transcript session/page state.
- [x] 6.8 Explicitly classify reconstruction-only, descriptor, request-audit, approval-audit, feedback, and other audit-only events as ignored after required state is consumed.

## 7. Bridge History and Wire Contract

- [x] 7.1 Expand `bridge/protocol-contract.json` with only the event families required to reconstruct supported displays/accessories, bump protocol compatibility as needed, and regenerate `docs/protocol.md`.
- [x] 7.2 Update bridge history caching/paging so live and cold sessions preserve event order and all event-level surface metadata for the expanded roster.
- [x] 7.3 Define bounded bridge projections/trimming for supported rich tool metadata without restoring unrestricted tool-result payloads.
- [x] 7.4 Add bridge contract/history/trim tests for the expanded roster, repeated surface replacement fixtures, cold persistence, caps, and audit-only omission.

## 8. Performance, Cleanup, and Validation

- [x] 8.1 Replace remaining reverse full-list lifecycle scans with projector correlation indexes and add long-session regression coverage.
- [x] 8.2 Verify streaming tail updates, structural mutation invalidation, animation redraw stopping, visible-window cloning, stable unit reuse, and copy provenance through cache/UI tests.
- [x] 8.3 Verify accessory and projector state updates obey the main-loop lock discipline, including queued dispatch, copy navigation, blocking questions, and session switches.
- [x] 8.4 Remove the temporary `Msg` compatibility conversion and obsolete event-specific UI branches after all existing presentations use the four shared surfaces.
- [x] 8.5 Update `README.md`, `AGENTS.md`, and `docs/design.md` with the display framework, supported event classes, interaction changes, protocol roster, and intentional audit-only exclusions.
- [x] 8.6 Run `cargo test`, `cd bridge && npm test`, protocol-doc generation checks, snapshot timing/smoke examples, and `node tools/smoke-bridge.mjs` against the deployed bridge copy.
