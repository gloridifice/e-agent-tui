# Implementation design

## 1. Baseline and goals

The checkout starts at a219feb with no uncommitted application changes. Current documentation is a renamed docs tree: root guides, subsystem architectures, history, and archive coexist under Doco. The existing current client and bridge documents supply the migration baseline, checked against owning source and tests. Historical OpenSpec specs are not requirements for this work.

The installed Doco context walker follows explicit Markdown and inline-path references, excluding tmp and unselected lifecycle packages but not arbitrary history folders. Merely labeling a linked history file Historical does not isolate it. Installed template dry-run also reports newline-only conflicts; template refresh is outside this migration.

## 2. Overall approach

Keep doco/architecture.md as the single concise system map. Extract durable frontend, rendering, interaction, Preview, configuration, session, and bridge contracts into doco/specs. Put the generated wire reference there with an explicit derivative header. Record the Doco-only authority/context decision under doco/decisions.

Move operational guides to readme. Move existing history/archive there and retain the superseded subsystem documentation as historical snapshots. Current documents may expose history directory links for deliberate human lookup, but must not link historical Markdown files into the default context graph. Do not manufacture completed or archived Doco packages for imported documents.

## 3. APIs and data model

No application API, state, persistence, concurrency, or protocol payload changes. The only executable edit changes the generated Markdown destination from doco/protocol.md to doco/specs/wire-protocol.md and adjusts its canonical-source link for the extra directory depth. Update the corresponding bridge test and source comment, then regenerate the embedded bridge mirror.

Doco state remains directory-derived. This selected package stays active with proposal.md and work/implement.md plus work/tasks.md. No duplicate lifecycle metadata is introduced.

## 4. Algorithms and rules

1. Read current source/tests and classify existing documentation by authority and lifespan.
2. Relocate guides and historical documents without deleting their content; repair relative links for new locations and label superseded architecture snapshots Historical.
3. Write the current architecture, focused contracts, decision, and navigation. Exact inventories/defaults stay in source/generated references. Investigate conflicting facts instead of declaring the code correct by default.
4. Update protocol generation and consumers, regenerate outputs, and validate.
5. Inspect doco context for this package; historical files, retired integrations, unrelated changes, and tmp must not enter its READ set.

Filesystem changes are local and sequential; refuse destination collisions. Preserve user-owned assets and existing historical requirements. Do not rely on Doco check to certify source/contract agreement or link-graph relevance.

## 5. Fixed decisions and discretion

Fixed: English maintained docs; Doco-only new changes; unchanged application requirements; history stays non-current; one current architecture owner; no automatic lifecycle transition. Guide filenames and contract grouping can be chosen to minimize duplication and broken links. No unresolved design blockers.

## 6. Verification and documentation impact

Run doco check normalize-doco-documentation, inspect its context output, validate local Markdown targets and moved-file preservation, run both protocol generator check entry points and release mirror check, and run bridge/test/protocol.test.js. Run git diff --check. No broad Rust checks or live DSH deployment are needed for documentation and path-only edits. All current documentation entry points, owning contracts, human guides, and historical labels are in scope; installed skill content is unchanged.
