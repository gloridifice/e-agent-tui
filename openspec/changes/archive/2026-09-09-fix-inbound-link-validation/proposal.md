## Why
Manual acceptance of #24 shows no tags. Both executable inbound loops execute ValidateLinks but drop EffectExecution.completed; controller-only tests manually delivered the result and missed the runner integration defect.

## What Changes
Reduce completion results from inbound and queued-action batches in both runners, as the terminal-action path already does. Add runner coverage and an execution-to-render regression.

## Capabilities
### New Capabilities
None.
### Modified Capabilities
None.

## Impact
Both Rust executable loops and focused tests. Checked `quick-link-copy` and the client executor ownership invariant: this restores delivery of already specified effect results without changing requirements. Specs are skipped.
