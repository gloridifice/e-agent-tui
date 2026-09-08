## Why

Normal-mode Preview currently treats user cards and plain system/error output as automatic targets even though those messages are already complete in the transcript, while direct command-result settlement can update an eligible activity without refreshing its Preview. This produces noisy target changes in one direction and stale content in the other.

## What Changes

- Exclude user-authored cards, user attachments, and plain transcript blocks (including local system/error blocks) from normal-mode automatic Preview following.
- Preserve explicit Reading View Preview behavior for those blocks.
- Keep specialized context, reasoning, tool, activity, and unknown-surface previews eligible.
- Give fallback Preview targets node-local revisions so unrelated ignored transcript appends do not refresh the current Preview.
- Reconcile Preview after a direct command result settles an existing activity.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `unified-preview-pane`: Narrow normal-mode automatic eligibility and require eligible direct activity updates to refresh the same stable Preview target.

## Impact

The change affects provider-neutral Preview target selection and transcript revision bookkeeping under `crates/e-tui`. It does not change DSH/Pi protocols, Preview rendering formats, Reading View navigation, persisted configuration, dependencies, or keybindings.
