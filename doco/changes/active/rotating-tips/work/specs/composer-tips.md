<!-- doco:managed template=v1 -->
# Composer Tips target specification

## Scope and current contracts

This is the target for a working-only Tips decoration in the existing composer
bottom rule and a provider-neutral content-registration boundary. It does not
change advertised commands, prompt admission, native history or storage.

Current [presentation](../../../../../specs/presentation.md) preserves shell
border labels, grapheme-safe layout and semantic copy. Current
[runtime contracts](../../../../../specs/runtime-and-adapters.md) require shared
deadline-driven policy and adapter-owned native effects. Current
[key-mapping rules](../../../../../specs/configuration-and-storage.md#key-mappings-and-themes)
require hints to use resolved bindings. Those contracts remain in force.

Tips are composer decoration, not normalized agent events. Execution was
authorized after catalog review, including a persistent Settings toggle. The
[implementation design](../implement.md) records the selected API, eligibility,
visibility and timing decisions; tasks track delivery and verification.

## Requirements

### Placement and styling

- Tips MUST render right-aligned inside the ordinary composer's existing bottom
  horizontal rule, without adding rows or changing editable input geometry.
- The displayed prefix MUST be `Tips` followed by exactly one ASCII U+0020
  space, then the localized body.
- Content MUST support multiple styled segments. Ordinary text MUST use the
  theme's activity-label tone; the label, keywords and effective key bindings
  MUST use its activity-detail tone (Umber and Bark in Ferra).
- Existing bottom-rule labels MUST receive width before Tips. Tips MUST have
  the lowest presentation priority and MUST NOT obscure or move those labels.
- Width reduction MUST clip at complete grapheme boundaries with terminal-cell
  widths and one trailing ellipsis, or omit the tip when insufficient useful
  width remains. Tips MUST NOT wrap.

### Visibility and rotation

- A default-on `show_tips` Display setting MUST enable or disable Tips through
  the existing Settings/config workflow in both languages and frontends. Changes
  and reload MUST apply live. Disabling MUST remove presentation and its deadline;
  re-enabling MUST start a fresh interval. Only this preference is persistent.
- Tips MUST be eligible only during live agent work with an ordinary composer
  visible. Idle, blocking decisions, replacement Input Pages and full-screen or
  modal views MUST hide them.
- The selected tip MUST change periodically while visible. Hidden time MUST
  NOT consume its reading interval. Idle MUST remove the Tips-specific deadline.
- A new working period MUST start a fresh interval. Loss of selected-candidate
  eligibility MUST NOT leave stale or unsupported advice visible.
- Definitions MUST survive session replacement within one frontend instance;
  session-specific eligibility and rotation MUST reset. No rotation state is
  persistent.
- A held visual selection MUST retain its committed screen snapshot until
  release. Tips MUST NOT bypass frame admission or selection suspension.

### Content and registration

- e-tui MUST own the public registration API and common definitions. Each
  adapter MUST register only its own private definitions at composition.
- The shared renderer MUST consume a combined registry without backend enums,
  provider wire names, imports of adapter crates or private-catalog copies.
- Definition IDs MUST be unique. Registration MUST validate the full batch and
  return a typed error without partial mutation on invalid or duplicate content.
- Registration MUST NOT silently replace another owner's definition.
- Effective key mappings MUST supply shortcut text. Disabled required actions,
  unavailable commands/capabilities/prerequisites and missing translations MUST
  exclude the corresponding tip.
- A private ID namespace or command name alone MUST NOT establish capability.
  Availability MUST come from normalized state supplied by its current owner.
- The user-reviewed Chinese strings in [the catalog](../catalog.zh-CN.md) define
  the selected content, with English equivalents. Held macro candidates MUST remain disabled until their
  advertised workflow is accepted for delivery.

## Boundaries, errors and compatibility

Tips MUST NOT mutate the draft, prompt parts, cursor/IME positions, transcript
source, whole-message copy or backend history. They MUST NOT trigger native
operations, filesystem reads or command execution. Visual-cell selection keeps
its existing contract; seeing a tip does not make it an agent message.

Timing MUST be monotonic shared presentation policy, not an idle polling loop
or independent executable timers. Drawing MUST remain free of I/O and MUST NOT
advance rotation. No backend protocol or persistent schema change is implied.

Invalid definitions include empty IDs, duplicate IDs/locales, empty bodies and
multiline/control-bearing text. Missing translations or capabilities are normal
ineligibility, not fatal runtime errors. Invalid built-in registration must be
reported during startup, not deferred into a rendering failure.

## Acceptance scenarios

### A1: working, idle and hidden views

With eligible content and ordinary composer work active, show a tip in the
bottom rule. Settle the task or open a blocking decision, replacement page or
modal: no tip is visible. Return to visible work: resume eligible presentation
without charging the hidden period against reading time. Idle has no Tips wakeup.

### A2: lowest-priority width allocation

With a shell label or exact macro label present, reduce composer width. Keep its
existing position and priority. Clip Tips without splitting a CJK glyph,
combining sequence or emoji cluster, and then omit Tips when space is too small.
Input rows and cursor positioning are unchanged. The prefix has one space.

### A3: registration isolation and rejection

Compose e-dsh with shared plus dshe definitions, and e-pi with shared plus pie
definitions. Neither shows the other's private content. Register a batch with a
duplicate ID or invalid body: return a typed error and retain the exact previous
registry. Rendering performs no backend-identity selection.

### A4: bindings, locale and availability

Remap a shortcut and reload: its candidate shows the effective binding. Disable
it: exclude the candidate. Remove a required command or capability, or select a
locale without a translation: do not show stale, unsupported or mixed-language
advice. A non-native `mcp` command does not enable native-manager Tips.

### A5: deadlines and session replacement

Allow the visible interval to expire: advance one candidate. Hide the line:
stop its deadline without periodic waking. Replace the session: retain registered
definitions and reset session-dependent eligibility and rotation. No transcript
nodes, semantic cache rebuilds or persisted records are created by rotation.

### A6: input, copy and selection

Edit multiline/atomic input while Tips rotate: input and cursor remain unchanged.
Copy a semantic message: Tips are absent. Hold visual selection across a due
rotation: the committed snapshot stays fixed until release, then live rendering
resumes through existing frame admission.

## Current-document impact

After delivery, synchronize approved facts into current architecture,
presentation, and runtime/adapter contracts. This preparation does not update
those documents. Config/storage must document the approved persistent `show_tips` preference. Do not modify README files.
