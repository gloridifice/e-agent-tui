# Share path-completion policy

## Purpose

Both executable adapters implement the same `@` completion policy independently. Directory scanning, name filtering, candidate ordering, and truncation were introduced together in both adapters. Consequently, changing prefix matching to substring matching currently requires editing the same user-visible rule twice.

Give the provider-neutral completion policy one owner while keeping filesystem effects in the adapters. Deliver the requested case-insensitive substring matching through that shared policy.

## Scope and acceptance

- Move name eligibility/matching, directory-first ordering, and the result limit into `e-tui::path_completion`; both adapters must use that implementation.
- Match the unfinished name within the selected directory using a case-insensitive contiguous substring, not prefix-only or subsequence matching. Given the directory 'readme/', the file 'README.md', and unrelated entries, both `@ea` and `@EA` retain exactly 'readme/' and 'README.md', in that order.
- Preserve original candidate spelling, directory suffixes, existing directory navigation, workspace selection, path restrictions, quoting, result correlation, and bounded enumeration. Preserve the existing case-sensitive lexical tie-breaker within the directory/file groups.
- Keep directory reads, file-type and symlink probes, native path interpretation, and blocking-task scheduling adapter-owned and outside frontend state locks. Do not introduce adapter-to-adapter dependencies or filesystem effects into `e-tui`.
- Test the matching/ordering policy once in `e-tui`, with small real-filesystem integration checks for both adapter callers.
- On delivery, document the matching semantics and policy/effect ownership in the current interaction and runtime contracts. No transport or persistent-format change is intended.

Non-goals: deduplicating all adapter filesystem code; moving quick-link validation; adding a shared infrastructure crate; changing completion keys; recursive search; fuzzy ranking; fuzzy matching of parent path components; changing exact-directory auto-descent; changing Unicode normalization beyond the existing lowercase convention.

This package records the approved implementation design. Completion and archive remain separate operations. See [implementation design](work/implement.md) and [tasks](work/tasks.md).

## Result

Delivered — `e-tui` now owns case-insensitive contiguous-substring matching, candidate safety, ordering, and limits; DSH and Pi retain filesystem enumeration and probes. Current interaction/runtime contracts were synchronized. Verified with `cargo fmt --all --check`, scoped `e-tui`/adapter path-completion tests, both adapter architecture tests, and `doco check share-path-completion-policy`; all passed.
