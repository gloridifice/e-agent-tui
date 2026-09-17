# Implementation design

## 1. Baseline and goals

Current entry points:

- [Shared completion module](../../../../../crates/e-tui/src/path_completion.rs): owned `PathCompletionRequest` and `PathCandidate` values; token recognition, slash normalization, and safe composer replacement. It currently contains no candidate selection policy or filesystem operations.
- The `complete` functions in [DSH completion](../../../../../crates/e-dsh/src/path_completion.rs) and [Pi completion](../../../../../crates/e-pi/src/path_completion.rs): duplicate directory resolution, bounded enumeration, lowercase prefix matching, unsafe-name rejection, candidate construction, directory-first sorting, and truncation.
- `ProductionRuntimePorts` in [DSH runtime ports](../../../../../crates/e-dsh/src/runtime_ports.rs) and `PiRuntimePorts` in the [Pi executable](../../../../../crates/e-pi/src/main.rs): execute completion in `spawn_blocking` and return owned results through `UiActionPorts`.
- `complete_paths` in the [frontend input module](../../../../../crates/e-tui/src/input.rs): rejects stale results and constructs suggestions; it does not re-filter adapter candidates.

Git locator `8bf919e` introduced both completion implementations together. The current functions have no provider-specific matching behavior. The architectural requirement to keep filesystem effects outside `e-tui` explains the two effect entry points, but does not require two copies of pure candidate policy.

Diagnosis: duplicated policy (The Pragmatic Programmer, DRY) makes one interaction change require synchronized adapter edits and risks backend behavior drift. The dependency direction itself is correct. Existing runtime ports provide the effect seam; no new filesystem abstraction is required. Team ownership information is unavailable and is not assumed.

Relevant uncommitted baseline: each adapter has a newly added `completion_matches_case_insensitive_name_substrings` test. Both fail at `ea` with an empty result instead of the directory 'readme/' followed by the file 'README.md'. No production code has been changed. During implementation, consolidate the broad policy matrix into shared tests and retain a small filesystem integration check in each adapter rather than maintaining two complete policy suites.

## 2. Overall approach

Dependencies remain:

```text
 e-dsh ───> e-tui <─── e-pi
   │                    │
   └─ native filesystem ┘
```

Shared ownership in `e-tui::path_completion`:

- Name eligibility: reject control characters and quotes.
- Case normalization and contiguous substring matching.
- Candidate ordering and the existing maximum returned-candidate count.

Adapter ownership:

- Native lexical path restrictions and selection of the directory to enumerate.
- The existing scan bound, directory reads, file types, and symlink-directory probes.
- Construction of project-relative `path` and leaf `label` with directory suffixes.
- Blocking-task dispatch, failures, and effect completion delivery.

Each adapter resolves `(directory, fragment)`, creates the shared matcher once, scans entries, checks each raw name using that matcher before file-type probes, builds candidates, and calls the shared finalizer. The finalizer receives owned values, never `ReadDir`, `DirEntry`, or an I/O callback. All of this remains inside the existing blocking task.

This deliberately merges policy, not the entire filesystem implementation. Remaining directory-resolution and enumeration code is explicit adapter-side duplication, deferred from this change.

## 3. APIs and data model

The existing request/candidate types and `UiActionPorts::complete_paths` signature remain unchanged. Add these pure APIs to `e-tui::path_completion`:

```rust
pub struct PathNameMatcher {
    needle: String,
}

impl PathNameMatcher {
    pub fn new(fragment: &str) -> Self;
    pub fn matches(&self, name: &str) -> bool;
}

pub fn finalize_candidates(candidates: Vec<PathCandidate>) -> Vec<PathCandidate>;
```

These are planned APIs, not existing ones.

- `new` stores the lowercase fragment once per request.
- `matches` accepts a raw leaf name, without the display-only directory suffix. It rejects unsafe names and checks lowercase `name.contains(needle)`. It is not a path parser.
- `finalize_candidates` accepts already matched candidates, sorts directory labels ending in a forward slash first, applies the existing lexical label tie-breaker, and truncates to the existing result limit of 100. Keep that limit private to the shared policy implementation.
- No new error type, cache, persistence, global state, dependency, or lifetime relationship is needed. The matcher and candidate vector are request-local owned state. The helper methods perform no filesystem operations.
- Adapter function signatures and public Pi completion entry points remain compatible. No manifests or package/release tooling changes are needed.

## 4. Algorithms and rules

1. Preserve native `Path` validation: only normal and current-directory components are accepted. Absolute paths, parent traversal, and unsupported native prefixes remain rejected.
2. Preserve directory resolution: if the full query names an existing directory, enumerate it with an empty fragment, even without a trailing slash. Otherwise enumerate its parent and match its final component. A missing directory query ending in a forward slash returns no candidates.
3. Preserve the first-4096-entry enumeration bound before filtering, including its existing behavior on unreadable entries. This change does not promise exhaustive search beyond that bound.
4. Convert each entry name to UTF-8; skip failed conversions. Apply the shared name matcher before reading its file type. This preserves the existing avoidance of metadata and symlink probes for unmatched names.
5. Preserve directory detection, including symlinks to directories, and project-relative candidate construction with forward slashes and original name spelling.
6. Run shared finalization only after matching and candidate construction. Do not truncate before sorting or before matching. An empty fragment includes every eligible scanned entry.
7. Preserve read/probe failure handling: unreadable directories produce an empty result; individual unreadable entries are skipped. Preserve the runtime ports' existing failed-worker fallback.

Matching examples:

- `ea`, `EA`, and `eA` match both 'readme' and 'README.md'.
- `mD` matches the suffix of 'README.md'.
- `rm` does not match 'README.md': noncontiguous subsequence matching is out of scope.
- The query 'readme/EA' matches 'README.md' inside 'readme', not a substring in the parent directory name.
- Parent path spelling continues to resolve according to the native filesystem; case-insensitive candidate matching does not make directory traversal case-insensitive on case-sensitive filesystems.
- Case handling remains Rust `str::to_lowercase`, not locale-sensitive comparison, full Unicode case folding, or Unicode normalization.

No controller, request-correlation, lock, editor-cursor, or input-key changes are required.

## 5. Fixed decisions and discretion

Design decisions for this proposed handoff:

- Reuse the existing frontend policy module; do not add another crate for this bounded extraction.
- Share eligibility, matching, ordering, and result truncation together, rather than sharing only a one-line substring helper and leaving surrounding policy duplicated.
- Do not move the entire adapter module into `e-tui`: it contains filesystem effects and unrelated quick-link validation, which would violate current contracts.
- Do not make either adapter import the other or share source through `include!`/cross-package `#[path]`.
- Do not add a generic completion-engine trait or a filesystem-provider abstraction; owned values and two pure policy APIs suffice.
- Keep filtering before metadata probes. Collecting all entries with metadata before a single shared batch filter would be simpler superficially but adds unnecessary filesystem work.

Local discretion: private constant naming, test helper names, and small fixture organization. There are no blocking design questions. Execution still requires the user's approval after this planning phase.

## 6. Verification and documentation impact

Shared tests should cover mixed-case prefix/middle/suffix matches, empty queries, rejected nonmatches/subsequences, Unicode names, unsafe names, unchanged candidate spelling, directory-first ordering, lexical tie-breaking, and the result limit after sorting. Test the full policy matrix only here.

Each adapter needs a small temporary-directory integration check proving that `ea`/`EA` reaches the shared policy and returns the expected original paths, plus a nested fragment check. Preserve existing Pi hierarchy tests for exact-directory descent, quoting-related names, empty/missing directories, and rejected paths. Reuse the existing frontend input tests for acceptance without sending and stale-result rejection.

Planned scoped checks:

```text
cargo test -p e-tui --lib path_completion
cargo test -p e-dsh -p e-pi --lib path_completion
cargo test -p e-dsh -p e-pi --test architecture
```

Run `doco check share-path-completion-policy` for the package. Review the diff for any residual candidate matching/sorting/truncation policy inside either adapter's `complete` function. No full workspace test suite is required. Before any later commit, follow the repository's formatting gate.

Update current documents only when the implementation is delivered:

- [Interaction contract](../../../../specs/interaction-and-sessions.md): case-insensitive substring matching of the unfinished name within the selected directory, without changing navigation semantics.
- [Runtime contract](../../../../specs/runtime-and-adapters.md): shared pure completion policy with adapter-owned filesystem enumeration/probes and out-of-lock dispatch.

The [current architecture](../../../../architecture.md) already assigns provider-neutral interaction to `e-tui` and filesystem effects to adapters; its dependency map remains accurate. No architecture rewrite, ADR, README/key-reference change, wire update, or persistent-format migration is needed. During planning, current contracts remain untouched.
