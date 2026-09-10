## 1. Implementation

- [x] 1.1 Replace full-file indexing with ordered candidate enumeration and bounded reverse-name/fallback reads; test pagination beyond 500 files, ordering, large records, malformed metadata, name clearing, and project filtering.
- [x] 1.2 Add neutral Resume paging demand and stale-result admission, run filesystem work outside the Pi event loop, and preserve DSH listing; test viewport-sized batches, lazy continuation, progressive search, and page/workspace isolation.
- [x] 1.3 Render path-free title/date rows with reserved date width and persistent title/date tones; test row geometry, clipping, absent dates, and localized loading versus completed empty search.
- [x] 1.4 Document the changed asynchronous boundary and run scoped regression tests plus workspace formatting/Clippy, architecture checks, and delta validation.

Verification: Pi session-index tests (6), Resume-related frontend tests (9), Input Page tests (24), page rendering tests (12), Pi CLI tests (2), DSH adapter tests (16), and architecture checks (16) passed. Workspace formatting passed; workspace all-target Clippy completed with existing warnings. Spec synchronization follows through the repository's CLI archive workflow.
