## Context

The built-in `/help` command currently mutates `InteractionModel.help_visible`; transcript rendering then substitutes `ui::overlay::help_overlay` for normal message content. `e-tui` already has a canonical Markdown `TranscriptBlock` surface, automatic Markdown materialization, copy provenance, Reading View support, and local plain-message insertion. Provider requests are produced separately as `AgentRequest` values.

## Goals / Non-Goals

**Goals:**
- Append `/help` output as complete, non-streaming Markdown in the canonical transcript.
- Keep the output frontend-only and absent from DSH/Pi requests, provider history, and model context.
- Derive command entries from the authoritative built-in catalog and the current integrated command roster.
- Preserve the existing `Ctrl+H` quick overlay.

**Non-Goals:**
- Persisting help across process or session switches.
- Changing the DSH wire contract, bridge command execution, or Pi RPC protocol.
- Replacing the `Ctrl+H` overlay or changing keybindings.

## Decisions

1. **Insert a local Markdown display block directly into `RuntimeState`.** Add a focused local Markdown insertion method that creates a unique `DisplayId`, uses `TranscriptFormat::Markdown`, sets `streaming` false, and invalidates the transcript cache. This reuses the canonical display surface without fabricating an agent timeline event.

2. **Let Markdown materialization allocate provenance units.** The new block starts with `unit: None`, matching assistant Markdown. `presentation::materialize_transcript` remains authoritative for semantic lines and copy units. Preallocating one plain-text unit was rejected because Markdown may produce multiple semantic units.

3. **Keep `/help` on the local command path with no outbound outcome.** `CommandAction::Help` builds the Markdown from the current catalog and inserts it while leaving `CommandOutcome.outbound` empty. Sending a bridge command or Pi prompt was rejected because it would make context exclusion provider-dependent.

4. **Generate help in a provider-neutral leaf module.** A small help-content module owns the static interaction sections and formats built-in plus integrated command descriptors. Built-in collision behavior reuses the existing merged catalog so the output matches command completion.

5. **Preserve `Ctrl+H` overlay semantics.** Only `/help` changes. The command dispatcher no longer needs `help_visible` in `LocalCommandContext`; terminal routing and overlay rendering retain that state independently.

## Risks / Trade-offs

- **[Help is local and disappears on session reset]** → This is intentional; provider persistence would violate context isolation.
- **[A large integrated command roster creates a long message]** → The transcript already scrolls and the roster is the same user-visible catalog used by completion.
- **[Static key guidance can drift from the overlay]** → Keep command names catalog-derived and cover representative key/help content with UI tests; a future refactor can share a richer semantic help model if both presentations evolve together.
- **[Markdown could accidentally start paced reveal]** → Use `streaming: false` and do not create a reveal track.
