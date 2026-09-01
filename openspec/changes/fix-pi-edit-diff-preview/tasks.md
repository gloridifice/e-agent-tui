## 1. Normalized Mutation Facts

- [x] 1.1 Add a provider-neutral event-authored unified mutation diff to tool-result facts and staged result state.
- [x] 1.2 Make settled Preview reduction prioritize a unified mutation diff while preserving existing hunk and call-time fallback behavior.

## 2. Pi Edit Normalization

- [x] 2.1 Normalize current and legacy Pi edit call arguments into ordered mutation hunks with safe path/JSON fallback.
- [x] 2.2 Normalize `details.patch` from live and replayed Pi edit results into the authoritative unified mutation diff.

## 3. File Projection and Verification

- [x] 3.1 Recognize non-empty, single-path mutation hunk references as foldable file activities.
- [x] 3.2 Add focused adapter/projection/reducer regression tests for pending, settled, legacy, malformed, and result-before-call behavior.
- [x] 3.3 Run scoped Rust formatting and tests for `e-pi` adapter and affected `e-tui` projection/Preview modules.
