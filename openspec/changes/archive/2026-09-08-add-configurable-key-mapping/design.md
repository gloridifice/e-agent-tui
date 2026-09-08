## Context

The frontend routes terminal events centrally, then composer, Reading, page, login, settings, and approval handlers interpret hardcoded KeyEvents. Config filesystem work belongs to the two adapters. Windows already normalizes raw VT with a physical snapshot and emits a Ctrl+V fallback; this transport must remain independent of binding policy.

## Goals / Non-Goals

**Goals:** Complete scoped keyboard customization, one TOML source of default bindings, exact modifier matching, atomic reload, effective-binding help, and unchanged semantic editor/runtime boundaries.

**Non-Goals:** Multi-step chords, arbitrary command macros, mouse remapping, terminal/OS shortcut interception, provider protocol changes, or expanding page editors into a new text editor.

## Decisions

- A pure leaf `key_mapping` module owns typed scopes/actions, parsing, normalized chords, effective context resolution and display. The root default TOML is embedded; adapters read only their own user file. Runtime mappings and diagnostics are skipped Config fields so persistence cannot rewrite key mappings.
- Strings and arrays replace a complete action binding. `nop` and empty arrays disable it. Unknown fields, invalid chords and conflicts reject the complete overlay with an actionable field path. Startup retains defaults; reload retains the previous valid map. Omitted files reset to defaults.
- Explicit context composition, not arbitrary dotted-table inheritance, defines ownership: ordinary messages combine message/edit and idle or working; suggestions and search replace overlapping modal actions; Reading Items inherit exit/copy and replace directional movement; page edit, choice, resume and question are separate from browse letter navigation. Disabled child actions shadow parent bindings. Global/local collisions fail except deliberate same-action help close/toggle. Global page-up/page-down actions are inactive when Reading owns input; Reading fast movement has independent bindings and cannot fall back to global paging when disabled. Fast movement executes 15 ordinary semantic cursor steps, checking the current Block/Item mode on each step and retaining existing viewport following.
- Physical events resolve to semantic actions, never synthetic legacy keys. Text falls through only in active text contexts and only without Ctrl/Alt/Super. An action is consumed once, even when unavailable. Global page-opening actions are ignored while another page, approval, or Reading owns input, protecting drafts and pending responses.
- Defaults follow the agreed inventory: osmain-H/R/L/E/comma/N/P, message Enter/Shift-Enter/osmain-Enter, literal Ctrl-C, existing word deletion aliases and Alt-Enter; unbound history search; Reading Esc/q exits and Backspace leaves Items; explicit approval y/Y and n/N/Esc.
- Windows Ctrl+V fallback remains a normalized key and passes through the same mapping lookup; removing the default binding prevents clipboard reads. Bracketed paste remains a separate text event and is suppressed in Reading.
- Help and footer rendering consume mapping labels rather than hardcoded shortcut strings. `/reload` updates bindings and subsequent hints without reparsing transcript layout per key.

## Risks / Trade-offs

- Terminal interception or ambiguous legacy encodings → document limitations and keep VT regression coverage; offer user remapping rather than promise unavailable keys.
- Scope conflicts and lost text → validate active context combinations, test disabled/remapped keys and protected editors, preserve complete draft payloads.
- Cross-cutting migration → migrate semantic handlers in stages, keep scoped regression tests, run workspace formatting, Clippy and architecture gates before completion.
- Existing specs contain older candidate Reading bindings → update the affected normative requirements instead of preserving obsolete Ctrl+V expectations.

## Migration Plan

Ship the embedded default with both executables; user files are optional and never auto-overwritten. `/reload` validates before replacing the map. Removing the user file restores defaults on reload. Rollback removes the new user file and reinstalls the preceding executable.

## Open Questions

None blocking. The agreed safer approval and Reading exit behaviors are intentional changes; empty arrays are accepted as an alias for `nop`.
