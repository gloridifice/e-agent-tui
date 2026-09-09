## Why
Command help text such as `/opsx-apply <other>` currently marks the slash-prefixed command as a filesystem target. A space-separated command placeholder is prose syntax, not one link target; quoted text remains eligible as one explicit target.

## What Changes
Treat slash-command tokens immediately followed by an angle-bracket placeholder as command syntax during discovery, while preserving quoted candidates and ordinary absolute paths.

## Capabilities
### New Capabilities
None.
### Modified Capabilities
None.

## Impact
Pure quick-link candidate discovery and focused regression tests. This is a clarification of target boundaries, so no spec delta is needed.
