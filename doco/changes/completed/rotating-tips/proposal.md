<!-- doco:lifecycle v=1 created-at=- completed-at=2026-10-15T10:12:34Z archived-at=- -->
<!-- doco:managed template=v1 -->
# rotating-tips

## Purpose

Useful frontend features are easy to miss while an agent is working. Show short,
rotating usage tips without adding another message or taking a row from the
conversation. Common advice belongs to e-tui; backend-specific advice belongs
to the executable that supports it.

The reviewed visual prototype places Tips on the right side of the composer's
bottom horizontal rule. It replaces the earlier idea of a separate row above
the input. The approved refinement displays only the localized content, without
a `Tips` label.

## Scope and acceptance

Deliver a provider-neutral Tips registry and composer presentation in e-tui,
with shared definitions initialized once and a public registration interface
for e-dsh and e-pi to append their own definitions. Both executables must use the
same styling, eligibility, layout and rotation policy.

The approved bilingual catalog contains 28 shared candidates, 4 dshe-only
candidates and 11 pie-only candidates. Long compound hints become independent
short tips, repeated advice is deduplicated, and unnecessary state/imperative
lead-ins are removed. Preview, queue cancellation/interruption and session-tree
hints are excluded at the user's request. Two prompt-macro candidates remain
held until that workflow is accepted.

Acceptance:

- During live agent work, an eligible tip appears right-aligned inside the
  existing bottom input rule. Idle and blocking user-decision states show no tip.
- Display only the localized body, without a `Tips` prefix. All content uses
  the theme's activity-detail color (Bark in Ferra); keywords and key bindings
  are bold and italic, while ordinary text is unaccented. Content supports
  multiple styled segments.
- Tips have the lowest presentation priority. Existing rule labels retain their
  width and position; Tips clip on complete graphemes or disappear when too
  little width remains. Tips never wrap or add composer height.
- New live work waits three seconds, shows a tip for fifteen seconds, then
  hides it for thirty seconds before showing the next candidate. Repeat until
  task settlement. Unavailable composer views pause the phase, while intentional
  delay/gap phases retain their deadlines. Idle has no Tips wakeup. Timing
  remains deadline-driven shared policy.
- Each executable registers only its own additional definitions. The shared
  renderer neither branches on backend identity nor imports either adapter.
- Shortcuts use the effective key mapping. Unsupported commands, capabilities,
  prerequisites and missing translations exclude their candidates.
- Tips do not alter input, cursor/IME positioning, transcript source, native
  history, prompt admission or whole-message copy. Visual selection keeps its
  committed snapshot unchanged.

Contract changes concern composer presentation, the public registry boundary,
shared scheduling and the `show_tips` preference. Synchronize current architecture and specs only
after those changes are implemented and approved.

Non-goals: implementing the advertised commands, changing backend behavior,
persisting rotation state, user-authored Tips, telemetry, new interaction keys,
README changes, or completing the separate prompt-macro work. A default-on
`show_tips` setting in Settings controls presentation and persists through the
existing frontend config workflow; disabling it removes the Tips deadline.
New or modified tests require separate user authorization.

The user authorized execution after revising the Chinese catalog and requested
a Settings toggle. Implementation decisions are recorded in work/implement.md.
The user later authorized completion after the remaining live acceptance limits
were disclosed; archive remains a separate action.

## Result

Delivered a shared provider-neutral Tips registry, adapter-owned catalog additions,
and the default-on Settings toggle. The refined 43-entry bilingual catalog omits
the three excluded hints. Presentation now waits three seconds, displays each
tip for fifteen seconds with thirty-second gaps, shows no `Tips` prefix, and
uses Bark text with bold-italic emphasis. Current architecture and contracts
are synchronized; no README files or tests were changed.

Both adapters compile. Twenty scoped existing tests, workspace formatting,
mechanical Doco checks, and an isolated production-library terminal inspection
passed. The inspection covered both catalog compositions, timing, hiding,
resume after Help, language, and text styling. Live Pi/DSH end-to-end runs and
specific registration, Unicode, capability, session-replacement, and selection
scenarios were not performed. The user explicitly accepted these disclosed
verification limits when authorizing completion; they are not reported as
passing checks. Archive remains unauthorized.
