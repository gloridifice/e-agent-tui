## Why
Quick-copy discovery treats C++ scope-qualified identifiers as URIs and standalone `//` as an absolute path, adding misleading tags to code.

## What Changes
- Reject separator-only targets and C++ scope syntax at URI classification.
- Preserve actual URI schemes, absolute paths, and link discovery inside code.
- Add focused regression tests for the reported code and supported Markdown contexts.

## Impact
- Affected capability: `quick-link-copy` (latest completed Markdown targets).
- Implementation: `crates/e-tui/src/link_copy.rs`; no adapter or persistent-format changes.
