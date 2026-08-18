> **状态：已完成并由 `remediate-architecture-audit` 收口。** 本变更的四公共表面与事件投影目标已保留；
> 临时 `Msg` compatibility conversion 已删除。生产 `AppState` 只保存 `TranscriptStore`，
> `client/tests/architecture.rs::single_track_transcript_has_no_legacy_production_path` 持续守卫这一状态。

## Why

DSH session events are currently reduced directly into ad-hoc transcript messages, which makes new event support inconsistent and leaves important semantics such as surface replacement, retries, todos, nested tools, and rich content unimplemented. A small set of shared display surfaces will make event coverage incremental while keeping transcript layout, copy provenance, input interaction, and rendering-cache behavior consistent.

## What Changes

- Introduce four composable display surfaces: status-bearing activity rows, ordinary transcript blocks, padded content cards, and input-area accessories.
- Classify DSH events at the typed protocol boundary and project them into those surfaces instead of adding event-specific rendering branches directly to the UI.
- Migrate existing Thinking, tool, user, assistant Markdown, lifecycle notice, approval, question, and queued-message presentations onto the shared surfaces without changing their visible behavior.
- Add projection support for currently missing user-visible event families, including todo state, model retry, command lifecycle/history, richer turn outcomes, context forms, and nested tool/workflow activity.
- Separate non-display semantics from display items: apply `surfaceOp` mutations before rendering, keep title/session metrics/page state outside the transcript, and ignore audit-only events by policy.
- Allow activity rows to reference parents so Code Mode and workflow activity can be represented incrementally; rich details may compose an activity row with a content card.
- Preserve unknown append-surface events through a bounded ordinary-transcript fallback.
- Update the shared wire contract when additional history event types are required and keep bridge/client compatibility explicit.

## Capabilities

### New Capabilities
- `event-display-surfaces`: Defines the four shared display surfaces, their composition rules, interaction behavior, copy provenance, and rendering-cache requirements.
- `dsh-event-projection`: Defines how DSH session events, surface mutations, page state, input accessories, unknown events, and audit-only records are classified and reduced.

### Modified Capabilities

None. There are no existing OpenSpec capability specifications.

## Impact

- Client protocol and projection: `client/src/protocol.rs`, `client/src/model.rs`, and new focused display/projection modules.
- Client rendering and interaction: `client/src/ui.rs`, `client/src/presentation.rs`, `client/src/copy.rs`, input handling, and transcript caching.
- Bridge history projection and wire roster: `bridge/src/history.js`, `bridge/protocol-contract.json`, protocol tests, and generated `docs/protocol.md`.
- Tests will need event-family reducer coverage, UI snapshot/layout coverage, surface-replacement/history coverage, and performance/cache regression coverage.
- User-facing behavior and architecture documentation will be synchronized in `README.md`, `AGENTS.md`, and `docs/design.md` during implementation.
