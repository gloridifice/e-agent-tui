## Why
Issue #22 reports twitching code blocks in streaming Preview. Generated fence line-count headers change near the start of the rendered signature, causing row reveal to discard and replay already visible code on append-only source growth.

## What Changes
Preserve Preview's painted frontier for append-only Markdown-family source revisions, while retaining common-prefix reconciliation for actual replacements and existing row pacing.

## Capabilities
### New Capabilities
None.
### Modified Capabilities
None.

## Impact
Shared Preview reveal sidecar and renderer, with a streaming fenced-code regression. Checked `paced-text-reveal` and `unified-preview-pane`: this restores same-target visible-prefix retention without changing pacing, centering, semantic source, or copy requirements. Specs are skipped.
