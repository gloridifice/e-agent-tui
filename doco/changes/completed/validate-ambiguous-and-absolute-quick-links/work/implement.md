# Implementation design

## 1. Baseline and goals

The frontend implementation is [`crates/e-tui/src/link_copy.rs`](../../../../../crates/e-tui/src/link_copy.rs):

- `discover` and `scan_text` extract a flat `Vec<LinkCandidate>` without filesystem I/O.
- `LinkCandidate.relative: Option<String>` currently overloads two meanings: `Some` is workspace-relative, while `None` is either a URI or an absolute path.
- `LinkCopyState::select` creates `UiAction::ValidateLinks`; `complete` filters the returned positional `PathValidation` values, deduplicates accepted targets, and assigns tags.
- `annotate` finds accepted target strings in rendered lines and inserts presentation-only suffixes.

[`crates/e-pi/src/path_completion.rs`](../../../../../crates/e-pi/src/path_completion.rs) and [`crates/e-dsh/src/path_completion.rs`](../../../../../crates/e-dsh/src/path_completion.rs) implement the adapter filesystem checks. They currently return `Exists` without probing whenever `relative` is `None`; relative paths are canonicalized against the workspace and rejected when lexical or symlink resolution escapes it. Both runtime ports execute the check through `spawn_blocking`. Generation, owner, source, and cwd checks already reject stale completion.

No relevant uncommitted source changes existed when this design was created.

The goal is to represent lexical uncertainty rather than prematurely choosing a token boundary, then use the existing bounded asynchronous validation path to select one real local target. Rendering and reducers remain filesystem-free.

## 2. Overall approach

The data flow becomes:

```text
completed Markdown
  -> pure discovery
  -> ordered candidate groups
       -> one URI candidate, or
       -> one unambiguous filesystem candidate, or
       -> several local-path hypotheses from a soft boundary
  -> one bounded ValidateLinks effect
  -> adapter probes filesystem candidates outside state guards
  -> frontend resolves each group
  -> global target deduplication and 36-tag assignment
  -> presentation-only annotation
```

Responsibilities remain:

- `e-tui::link_copy`: lexical classification, hypothesis grouping and ordering, missing-path policy, result resolution, target deduplication, tag assignment, and annotation boundaries.
- `e-pi::path_completion` and `e-dsh::path_completion`: native filesystem interpretation, existence checks, canonicalization, relative containment, and platform-specific rejection.
- Shared runtime executor and ports: unchanged effect ordering and blocking-task handoff.

No filesystem operation may run from discovery, completion reduction, layout, annotation, or rendering.

## 3. APIs and data model

Replace the overloaded `relative: Option<String>` with an explicit provider-neutral kind. Exact type names may vary locally, but the model must express these states without sentinel strings:

```rust
enum LinkTargetKind {
    Uri,
    AbsolutePath,
    WorkspaceRelative { normalized: String },
}

struct LinkCandidate {
    target: String,
    kind: LinkTargetKind,
    confidence: PathConfidence,
    retain_missing: bool,
}

struct LinkCandidateGroup {
    alternatives: Vec<LinkCandidate>, // most specific/longest first
    require_existing: bool,
}

struct LinkValidationRequest {
    generation: u64,
    cwd: String,
    groups: Vec<LinkCandidateGroup>,
}
```

Validation results must preserve group and alternative shape explicitly rather than relying on one flat candidate index. A small named result type is preferred over a bare nested vector:

```rust
struct CandidateGroupValidation {
    alternatives: Vec<PathValidation>,
}
```

`PathValidation` must distinguish a URI that needs no probe from an existing path, for example with `NotRequired`, `Exists`, `Missing`, and `Rejected`. A malformed or short result is treated as rejected; it must never retain a candidate accidentally.

`UiAction::ValidateLinks`, `EffectResult::LinksValidated`, and `UiActionPorts::validate_links` carry the grouped request/result. State ownership, generation checks, and asynchronous lifetime remain unchanged. These values are transient and are not persisted or sent over either provider protocol.

The selected `TaggedLink.target` remains the exact substring from source. Normalized relative paths and canonical absolute paths are probe-only values.

## 4. Algorithms and rules

### 4.1 Discovery and classification

Keep current Markdown event handling, quoted/code treatment, command-placeholder rejection, separator-only rejection, URI syntax guards, confidence rules, and ordinary-prose exclusion.

Classify in this precedence:

1. Windows drive/UNC/device or POSIX-rooted filesystem syntax as an absolute-path candidate.
2. Valid non-filesystem URI syntax as a URI candidate.
3. A normalized contained lexical relative candidate.

This precedence prevents a Windows drive prefix from becoming a URI scheme. Platform-neutral discovery may recognize foreign absolute syntax; the adapter decides whether it is native and probeable.

Define shared target-boundary helpers instead of separate discovery and annotation lists:

- Hard boundaries end a token as today: whitespace, quotes, backticks, angle brackets, and sentence punctuation.
- Soft local-path boundaries are Chinese/full-width opening wrappers used between a target and commentary. The initial supported set is `（`, `【`, `〔`, `《`, `〈`, `［`, and `｛`.

For a token containing soft boundaries, retain the complete token and derive prefixes ending before each soft boundary. Order unique hypotheses from longest to shortest, cap them at four per token, and keep only hypotheses that classify as filesystem candidates. URI candidates do not enter filesystem ambiguity groups; their existing lexical behavior remains unchanged.

A group is marked `require_existing` whenever a soft-boundary-derived hypothesis participated, even if the complete token failed classification and only one shorter hypothesis remains. This prevents `README（note）` from turning into a speculative missing README link.

Bounds apply before I/O: no more than 256 groups and no more than 256 total filesystem hypotheses may be sent for validation. Truncation preserves source order and alternative priority.

Do not globally deduplicate hypotheses during discovery. Resolve groups first, then deduplicate selected target strings in first-output order so duplicate targets still share one tag.

### 4.2 Adapter validation

URI alternatives return `NotRequired` without filesystem access.

For workspace-relative alternatives, retain the current algorithm:

1. Canonicalize the request cwd once when relative probes exist.
2. Reject non-normal lexical components and parent escape.
3. Join the normalized relative path to the canonical root.
4. Canonicalize the candidate or walk upward through missing tails.
5. Reject a resolved ancestor outside the root and reject missing children beneath an escaping or unreadable symlink.
6. Return `Exists`, `Missing`, or `Rejected`.

For absolute alternatives:

1. Require `std::path::Path::is_absolute()` on the running platform; foreign-platform syntax is `Rejected`.
2. On Windows, reject UNC prefixes and device/verbatim namespaces before any metadata or canonicalization call. They must not trigger network or device access.
3. Canonicalize the exact candidate. Success is `Exists`, `NotFound` is `Missing`, and permission, malformed path, loop, or other errors are `Rejected`.
4. Do not compare the resolved path with cwd. Existing absolute files and directories outside the workspace are valid.
5. Discard the canonical value after validation; copying preserves the original target text.

Both adapters must implement equivalent behavior and tests. All probes remain in their existing `spawn_blocking` effect and run after frontend locks are released.

### 4.3 Group resolution

Resolve each group independently:

1. A single URI with `NotRequired` is accepted.
2. Otherwise choose the first `Exists` alternative. Because alternatives are longest-first, an existing complete filename containing a soft wrapper wins over an existing shorter prefix.
3. If no alternative exists and `require_existing` is true, reject the whole group.
4. For an unambiguous relative group only, `Missing` may be accepted when the candidate's existing `retain_missing` rule permits it.
5. Absolute `Missing`, every `Rejected`, `NotRequired` on a filesystem candidate, and shape-mismatched results are rejected.

After group resolution, deduplicate by exact selected target, preserve first-output order, and assign only the first 36 tags from `1234567890abcdefghijklmnopqrstuvwxyz`. The existing stale generation/cwd/owner checks happen before any state mutation.

### 4.4 Annotation

The boundary predicate used by `annotate` must recognize the supported soft wrappers, allowing a chosen short target to receive its suffix before `（` or another wrapper. Existing source and copy payloads remain unchanged.

When selected target strings overlap at one displayed occurrence, select the longest matching target; use tag order only as a deterministic tie-break. Emit one suffix for that occurrence. This permits a short target to share one tag across exact occurrences without inserting both short and long tags inside a longer selected filename.

## 5. Fixed decisions and discretion

Fixed decisions:

- Absolute paths require existence; missing absolute paths are never retained.
- Existing native absolute paths outside the workspace are accepted.
- Relative paths retain workspace containment and the existing unambiguous strong-missing policy.
- Soft boundaries create hypotheses rather than unconditional token splits, and ambiguous groups require an existing result.
- Longest existing hypothesis wins; a group with no existing hypothesis produces no target.
- URI targets do not require filesystem validation.
- Windows UNC and device/verbatim namespace paths are rejected without probing.
- Selected text is copied exactly as authored.
- Candidate extraction and resolution remain in `e-tui`; filesystem effects remain in both adapters.

Local implementation discretion:

- Concrete helper and result type names.
- Whether the small alternative collections use `Vec` or a small-vector representation without adding a dependency.
- Internal organization of shared boundary predicates and overlap collection, provided all fixed ordering and bounds remain observable.

There are no blocking open questions.

## 6. Verification and documentation impact

Focused regression coverage must include:

- The reported `final-report/2_virtual_geometry_demo.html（+340/−7` case with only the short file existing.
- Complete-only, both-existing, and neither-existing soft-boundary outcomes.
- Tag insertion immediately before each supported soft wrapper and longest-match overlap behavior.
- Existing and missing native absolute paths, including an existing path outside cwd.
- Foreign-platform syntax and, on Windows, UNC/device rejection without probing.
- URI no-probe behavior, relative missing retention, lexical/symlink escape rejection, candidate/probe bounds, duplicate target tags, and stale completions.
- Equivalent adapter behavior in `e-pi` and `e-dsh`.

Use the narrow checks defined in [the execution tasks](tasks.md); broad workspace tests and Clippy are not required for this contained change.

On delivery, update the current [presentation contract](../../../../specs/presentation.md) with grouped target discovery and tag resolution, and the [runtime/adapter contract](../../../../specs/runtime-and-adapters.md) with adapter-owned absolute/relative validation. The current [architecture](../../../../architecture.md), interaction contracts, ADRs, operational guides, wire protocol, and user quick-reference documentation require no change.
