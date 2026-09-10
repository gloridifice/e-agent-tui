## Why

URI targets immediately following Chinese labels and a fullwidth colon are not discovered.

## What Changes

Treat the fullwidth colon as a prose token separator and add regression coverage for the reported links.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None.

## Impact

Only the pure scanner in `crates/e-tui/src/link_copy.rs` changes. Checked `openspec/specs/quick-link-copy/spec.md`: ordinary-prose URI discovery is already required. This restores that behavior without changing requirements; specs are skipped. ASCII colons remain part of URI schemes and Windows paths.
