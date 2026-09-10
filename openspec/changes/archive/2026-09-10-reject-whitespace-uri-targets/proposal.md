## Why

Inline-code commit subjects such as `feat: refine skill reads, preview and link targets` are incorrectly tagged as URIs.

## What Changes

Reject unencoded whitespace in URI classification without restricting valid custom schemes or filesystem paths containing spaces. Add focused discovery regression tests.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None.

## Impact

Only `crates/e-tui/src/link_copy.rs` is affected. Checked `openspec/specs/quick-link-copy/spec.md`: recognizing URI targets does not require treating prose containing unencoded whitespace as a URI. This restores existing target semantics; requirements are unchanged (`skip_specs: true`).
