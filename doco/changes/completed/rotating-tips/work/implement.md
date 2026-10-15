<!-- doco:managed template=v1 -->
# Implementation design

## 1. Baseline and goals

Baseline commit: `825b5a9`. Relevant current contracts are
[presentation](../../../../specs/presentation.md),
[runtime and adapters](../../../../specs/runtime-and-adapters.md),
[interaction and sessions](../../../../specs/interaction-and-sessions.md), and
[configuration and storage](../../../../specs/configuration-and-storage.md).
The target contract is [composer-tips](specs/composer-tips.md); content decisions
and source ownership are recorded in [catalog.zh-CN.md](catalog.zh-CN.md).

Current source entry points:

- [composer.rs](../../../../../crates/e-tui/src/ui/region/composer.rs) renders
  the composer and its bottom-rule shell or macro label.
  [pane/main.rs](../../../../../crates/e-tui/src/ui/pane/main.rs) composes it with
  existing accessories.
- [app.rs](../../../../../crates/e-tui/src/app.rs) owns `TuiApp`;
  [render_state.rs](../../../../../crates/e-tui/src/render_state.rs) holds
  presentation state. The app exposes presentation deadlines, and the shared
  runtime already provides deadline-driven waits and frame admission.
- [key_mapping.rs](../../../../../crates/e-tui/src/key_mapping.rs) resolves
  effective shortcut labels.
- [command_catalog.rs](../../../../../crates/e-tui/src/command_catalog.rs)
  declares common commands; [catalog.rs](../../../../../crates/e-tui/src/catalog.rs)
  stores normalized live catalogs.
- [e-dsh/main.rs](../../../../../crates/e-dsh/src/main.rs) and
  [e-pi/main.rs](../../../../../crates/e-pi/src/main.rs) construct runtime state
  before sharing it behind a guard. These are the registration entry points.

The original delivery established the production registry, timer and composer
integration. The approved refinement reuses those boundaries and replaces only
cadence, prefix presentation and bilingual content. Shell and macro labels keep
priority; held macro Tips remain excluded.

The retained [Python prototype](prototype/tips.py) illustrates the original
placement, not the refined cadence or prefix-free content. It is mock-only and
does not implement the complete catalog, live availability, native effects or
production scheduling. Its periodic refresh is not a precedent for a production
polling loop.

Run the retained source from the repository root:

```powershell
python -m pip install prompt_toolkit "wcwidth>=0.6"
python doco/changes/active/rotating-tips/work/prototype/tips.py
```

F2 toggles work/idle, F3 advances the tip, F4 switches border-label scenarios,
F5 switches language, F6 narrows the page, and Ctrl+Q exits. `--clean` hides the
prototype controls; `--interval` changes the mock rotation interval.

## 2. Overall approach

- e-tui owns common definitions, public registration, localization, styled
  segments, eligibility, rotation and composer decoration.
- e-dsh and e-pi own their private content and register it at startup.
- Common definitions initialize once in the default shared registry. Each
  executable appends its own batch before runtime state is guarded.
- Rendering consumes one registry, not separate backend queues. Opaque IDs such
  as `dshe.login-proxy` and `pie.fork` do not activate frontend backend branches.

Data flow: startup registration creates an owned registry; normalized live state
and the effective config select eligible candidates; shared presentation policy
advances the selection at a monotonic deadline; the composer borrows the selected
localized segments and resolves theme roles at the available border width.

Do not add agent messages or timeline nodes for these frontend-authored hints.
Use the existing composer region, not a new provider/event-specific surface.

## 3. APIs and data model

The public registration model has this shape; exact types and validation are
owned by crates/e-tui/src/tips.rs:

```rust
pub struct TipDefinition {
    pub id: String,
    pub translations: Vec<TipTranslation>,
    pub requirements: Vec<TipRequirement>,
}

pub struct TipTranslation {
    pub language: Language,
    pub segments: Vec<TipSegment>,
}

pub enum TipSegment {
    Text(String),
    Keyword(String),
    KeyBinding { scope: Scope, action: Action },
}

pub enum TipRequirement {
    Command(String),
    Capability(String),
    Prerequisite(TipPrerequisite),
}

impl TipRegistry {
    pub fn register_tips(
        &mut self,
        tips: impl IntoIterator<Item = TipDefinition>,
    ) -> Result<(), TipRegistrationError>;
}
```

Proposed composition:

```rust
// e-dsh: shared definitions are already in the default registry.
app.tips.register_tips(e_dsh::tips::definitions())?;

// e-pi: shared definitions are already in the default registry.
app.tips.register_tips(e_pi::tips::definitions())?;
```

Tip types, `app.tips`, and the adapter `tips` modules are delivered additions.
The API must own its definitions, accept batches and validate atomically. Reject
empty or duplicate IDs, duplicate locales, empty bodies, multiline text and
nonprinting control content with typed owned errors. A rejected batch must not
partially change the registry or overwrite common definitions.

Store semantic content, not RGB values, terminal escape codes, Ratatui spans,
callbacks, backend payloads or borrowed backend state. All segments use the
activity-detail color. `Text` is unaccented; `Keyword` and `KeyBinding` add bold
and italic modifiers.
Disabled bindings make the containing candidate ineligible. Bindings refresh
from the effective mapping after reload; they are not expanded at registration.

Locale lookup follows frontend language. A missing translation omits the tip;
do not expose raw translation keys or silently substitute another language.
The Chinese catalog's backticks denote Bark segments, not rendered Markdown.

### Availability decisions

`Command` checks normalized commands. `Capability` represents explicit normalized
availability, not a backend enum. Registration alone does not establish that a
feature is usable. Reuse current facts for completed answers, queues, local
shell work, quick-link tags, model marks, model efforts and MCP availability.
Do not create a second catalog or duplicate lifecycle state.

Use typed shared prerequisites for completed answers, admission barriers,
queues, local shell work, skills, selectable models/efforts, usable letter marks,
validated quick-link tags, attached sessions, selectable new modes, user messages,
native MCP availability and enabled fallback recovery. Read them directly from
their existing owners on each presentation reconciliation; do not cache a second
catalog. Command requirements read the builtin and normalized command catalogs.

Opaque capability keys cover adapter-owned support: `history`, `proxy-management`,
`compaction-routing`, `native-auth`, `session-ancestry`, `saved-session` and
`native-compaction`. Adapters set booleans through the public registry API before
shared reconciliation. DSH supplies its implemented history/proxy/compaction
support. Pi supplies history/ancestry/compaction support, saved status from native
get_state's sessionFile, and native authentication from a live helper matching
the native session plus the current logout command. Native MCP reads the existing
mcp_available fact, never a command-name heuristic. Capabilities reset on session
replacement and are republished by their owners; definitions survive. Reload
changes commands, effective bindings and catalogs in place, so eligibility is
recomputed without stale copies.

## 4. Algorithms and rules

Selected implementation rules:

1. Validate and append common then adapter definitions in stable declaration
   order. All requirements on a candidate must be satisfied.
2. Resolve language and effective bindings from current config and select the
   next eligible ID. Skip held macro candidates until accepted; no eligible
   candidates means no line and no Tips deadline.
3. Keep registry lifetime at the frontend-instance level, and rotation index,
   phase, remaining phase duration and deadline as ephemeral presentation state. Session
   replacement retains definitions but resets rotation and session eligibility.
4. Admit only live agent work with the ordinary composer visible. Hide for idle,
   blocking decisions, replacement Input Pages and full-screen/modal views;
   never infer work from old transcript activity or reveal alone.
5. Reserve existing bottom-rule labels before allocating Tips width. Display
   only the body, without a prefix. Clip complete graphemes with one trailing
   ellipsis, or suppress the tip if the remaining width is not useful.
6. Retain a 24-cell minimum content budget. Use initial-delay, visible and hidden
   phases with the durations in the target contract. The approved `show_tips`
   preference controls presentation; no timing/width or rotation settings are added.
7. New work begins with the initial delay. The visible phase ends by clearing
   the line; the following hidden phase ends by selecting the next eligible tip.
   Pause the remaining phase duration when the composer is unavailable. Advance
   at most one phase per due tick; never consume missed periods in a catch-up loop.
   A disappeared prerequisite invalidates its candidate without stale advice.
8. Schedule through shared monotonic presentation deadlines. Tick outside
   rendering and mark the composer dirty without rebuilding transcript caches.
   Intentional delays/gaps retain deadlines; unavailable views, disabled Tips and
   idle remove them. Both runners continue to use shared policy, not polling.
9. Visual mouse selection retains its immutable committed screen; defer live
   presentation updates until release without altering transcript semantics.

Production layout must consume one measured visibility/width decision for
rendering and visible-time accounting. It must not mutate the timer as a side
effect of drawing. Native operations, filesystem I/O and clipboard effects stay
with their current owners and outside frontend guards.

## 5. Fixed decisions and discretion

Approved by the user: bottom-rule right alignment; no extra row; Bark text with
bold-italic emphasis; prefix-free, phased working-only presentation; lowest
priority; a public e-tui registration boundary for each adapter's private content;
and bilingual content review before implementation.

The approved refinement replaces the original six-second rotation: initial
three-second delay, fifteen-second display and repeating thirty-second gaps.
Deliver 28 shared, 4 dshe and 11 pie candidates after splitting long hints and
removing Preview, queue cancellation/interruption and session-tree advice.
Exclude both held macro candidates. Retain the 24-cell minimum budget and owned,
atomic registration with semantic segments, typed prerequisites and capabilities.

Live work is current session working/running or live compaction, excluding a
new-session draft. Hide for approvals/questions, Input Pages, Help, MCP, Reading,
History, Preview-only layout, pane resizing, insufficient composer geometry,
disabled Settings or no eligible candidates. Held mouse selection suspends live
updates and reading time while retaining its committed snapshot. Compute the
bottom-rule width once in shared layout policy outside drawing, and reuse that
budget in rendering and time accounting. Preserve remaining phase time while
those views are unavailable; new work and re-enabling after disable restart the
initial delay. Removing idle qualifiers from copy does not relax native command
admission. Each split candidate retains only its own binding and availability
requirements.

The additional requested `show_tips` boolean defaults to true in embedded TOML,
appears in Settings Display in both languages, persists using existing adapter
config effects and applies live after editing or reload. It does not persist
selection, timing or catalog state.

Local discretion after approval: private module names, helpers and allocation
optimizations that preserve the target contract. No authority to complete or
archive this change follows from recording or committing it.

## 6. Verification and documentation impact

Preparation checks are recorded in [tasks.md](tasks.md). Future validation covers
registration isolation/rejection, remapped and disabled bindings, language
changes, work/idle and modal transitions, Unicode clipping, existing border
labels, deadline suspension, session replacement and immutable selection. See
[target acceptance scenarios](specs/composer-tips.md#acceptance-scenarios).

Use scoped build/lint and existing checks plus manual TUI inspection. Do not add
or modify tests without a separate user request. Read the project's testing
guide before choosing broad checks.

The delivered integration synchronizes current architecture for common/private
ownership, presentation for border placement and styling, runtime/adapter
contracts for registration and timing, and configuration/storage for `show_tips`. No README changes are authorized.
