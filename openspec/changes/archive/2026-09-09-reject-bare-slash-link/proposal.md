## Why

A standalone `/` is incorrectly tagged as a copyable link.

## What Changes

- Exclude exactly `/` from link candidates across supported Markdown contexts.
- Preserve other absolute paths and URI targets; add focused regression coverage.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `quick-link-copy`: exclude a standalone slash from target discovery.

## Impact

Shared frontend candidate classification in `crates/e-tui/src/link_copy.rs`. No adapter or filesystem validation changes.
