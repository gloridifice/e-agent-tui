# Approved Tips catalog

The user approved the revised Chinese and English copy together. The owning
source modules are authoritative for exact strings, IDs, bindings and eligibility:

- [Shared definitions](../../../../../crates/e-tui/src/tips/shared.rs): 28 candidates.
- [DSH definitions](../../../../../crates/e-dsh/src/tips.rs): 4 private candidates.
- [Pi definitions](../../../../../crates/e-pi/src/tips.rs): 11 private candidates.

## Content decisions

- Prefer command or shortcut followed by its purpose. Omit imperative lead-ins
  such as "Use", "Press", "Type" and their Chinese equivalents, and inferred
  state clauses such as "When idle" or "while the agent works".
- Split long compound hints into independently understandable, separately
  rotated candidates. Retain necessary menu/mode context and behavioral outcomes.
- Describe Reading copy once; the mouse-copy hint only describes mouse selection.
  The newline hint does not repeat the send action.
- Exclude Preview (`Ctrl+P`), queue cancellation/interruption and session-tree
  (`(fork)` in `/resume`) hints. Their actual commands and interactions remain
  unchanged.
- These are informational hints, not admission rules. Removing idle qualifiers
  does not change command restrictions. Keep exact-locale selection, effective
  key bindings and owner-supplied capability/prerequisite checks.
- Display the body without a `Tips` prefix. Backticks in source denote semantic
  keyword/binding segments, not visible Markdown.

## Held candidates

Prompt-macro definition and route-override hints remain outside this delivery.
Their separate workflow must be accepted before they become catalog candidates.

## Verification sources

Use the current [interaction contract](../../../../specs/interaction-and-sessions.md),
[default bindings](../../../../../crates/e-tui/assets/default_key_mapping.toml),
[command registry](../../../../../crates/e-tui/src/command_catalog.rs), and owning
adapter workflows to verify wording. Presentation and cadence belong to the
[target contract](specs/composer-tips.md), not this catalog summary.
