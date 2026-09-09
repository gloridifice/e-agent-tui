## Why
Issue #21 requests pane-local selection instead of copying across the full terminal width, which currently mixes transcript and Preview rows.

## What Changes
Bind each text gesture to its starting Main or Preview pane. Clamp horizontal drag endpoints and extract intermediate rows within that pane. Keep committed snapshots, Unicode extraction, clipboard delivery, and separator resize precedence unchanged.

## Capabilities
### New Capabilities
None.
### Modified Capabilities
- `application-mouse-selection`

## Impact
Selection frame geometry, pure mouse reducer, final-screen capture, regression tests, README mouse reference, and client architecture. Single-pane layouts retain viewport selection.
