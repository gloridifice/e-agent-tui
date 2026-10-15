<!-- doco:managed template=v1 -->
# rotating-tips

## Purpose

Useful frontend features are easy to miss while an agent is working. Show short,
rotating usage tips without adding another message or taking a row from the
conversation. Common advice belongs to e-tui; backend-specific advice belongs
to the executable that supports it.

The reviewed visual prototype places Tips on the right side of the composer's
bottom horizontal rule. It replaces the earlier idea of a separate row above
the input. The label and content are separated by exactly one ASCII space.

## Scope and acceptance

Deliver a provider-neutral Tips registry and composer presentation in e-tui,
with shared definitions initialized once and a public registration interface
for e-dsh and e-pi to append their own definitions. Both executables must use the
same styling, eligibility, layout and rotation policy.

The user-reviewed content draft contains 22 shared Chinese candidates, 3 dshe-only
candidates and 8 pie-only candidates. Two additional prompt-macro candidates
remain held until that workflow is accepted. The selected delivery includes
all 33 revised non-held candidates with English equivalents reviewed against
the current workflows during execution.

Acceptance:

- During live agent work, an eligible tip appears right-aligned inside the
  existing bottom input rule. Idle and blocking user-decision states show no tip.
- `Tips` is followed by one ASCII space. Ordinary content uses the theme's
  activity-label tone and keywords/key bindings use its activity-detail tone
  (Umber and Bark in Ferra). Content supports multiple styled segments.
- Tips have the lowest presentation priority. Existing rule labels retain their
  width and position; Tips clip on complete graphemes or disappear when too
  little width remains. Tips never wrap or add composer height.
- Visible tips rotate periodically. The interval is six visible
  seconds; hidden time does not consume reading time, and idle has no Tips
  wakeup. Timing remains deadline-driven shared policy.
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
Execution does not authorize completion or archive.

## Result

Implementation delivered, including the Settings toggle and revised bilingual
catalog. Scoped checks and isolated visual inspection are recorded in work/tasks.md;
full live acceptance follow-ups remain open. The change remains active and has not
been completed or archived.
