## Context

Normal-mode Preview scans the canonical transcript from newest to oldest, skips assistant Markdown and empty Thinking nodes, then builds a specialized or complete-source fallback target. Fallback revisions currently use the transcript-wide generation, so an unrelated append can refresh an unchanged earlier target. Host timeline reduction reconciles at transaction end, but direct `command-result` handling settles an activity outside that path and does not reconcile.

## Goals / Non-Goals

**Goals:**

- Make normal-mode automatic eligibility match the chosen content policy.
- Keep ignored appends from changing or replaying the current eligible Preview.
- Refresh a directly settled command activity under the same target identity.
- Preserve Reading View's explicit block/item Preview behavior.

**Non-Goals:**

- Remove plain text support from `PreviewContent` or Reading View.
- Change tool, reasoning, context, or unknown-surface rendering.
- Add new structured activity layouts.
- Change protocols, adapters, keybindings, or configuration.

## Decisions

1. **Apply exclusions only to normal-mode automatic following.** `TranscriptFormat::Plain`, `CardRole::User`, and `CardRole::Attachment` are skipped together with the already excluded assistant Markdown and empty Thinking nodes. Reading View continues to preview the block explicitly selected by the user.

2. **Store a node-local revision in `TranscriptStore`.** Insert and in-place touch assign the store's next generation to the affected node. Fallback Preview references use that revision rather than the transcript-wide generation, so appending an ignored node does not revise the previous candidate. This is preferred over hashing rendered content because it is collision-free within the store and follows existing mutation bookkeeping.

3. **Reconcile direct command settlement at the state mutation boundary.** `apply_command_result` reconciles after updating a correlated activity. Uncorrelated command output becomes an ignored plain system/error block and therefore does not replace the current target.

4. **Keep specialized revisions authoritative.** Entries in `preview_refs` continue to carry adapter/projection revisions; node-local revision is only the fallback source.

## Risks / Trade-offs

- **[Plain error details no longer appear automatically in Preview]** → They remain fully visible/copyable in the transcript and explicitly previewable in Reading View.
- **[Adding node-local revision changes an internal store record]** → Keep the field private to projection storage and cover insert/touch behavior with scoped tests.
- **[A user attachment is also skipped]** → This matches the requested user-message policy; context and skill cards remain eligible through their distinct roles.
