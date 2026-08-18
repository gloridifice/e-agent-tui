## 1. Canonical protocol

- [x] 1.1 Add the bounded `new-input{mode,text}` client shape and bump the canonical wire version
- [x] 1.2 Regenerate protocol docs, constants, package metadata, and Rust/Node fixtures
- [x] 1.3 Implement Rust serialization and Node dispatcher acceptance tests for `new-input`

## 2. Bridge materialization and history

- [x] 2.1 Route `new-input` through guarded new-session creation, attachment, and first followup
- [x] 2.2 Add typed blank-session classification using live events, projection cache, and persistence fallback
- [x] 2.3 Filter blank sessions before the history cap and title-enrichment frames
- [x] 2.4 Add Node regression tests for first-input routing, old-connection safety, cold/live blanks, and cap ordering

## 3. Client draft lifecycle

- [x] 3.1 Add typed pending-new state and display/copy overrides without replacing the retained real transcript
- [x] 3.2 Make `/new` update the local draft and reset only draft UI navigation without emitting a bridge command
- [x] 3.3 Route the first ordinary prompt to `new-input`, retain it during materialization, and restore it on creation failure
- [x] 3.4 Clear the draft only on a successful real session switch and keep `/resume` available
- [x] 3.5 Prevent session-scoped model/skill/integrated commands from mutating the retained old session
- [x] 3.6 Add controller and TestBackend regressions for no-send `/new`, `新对话`, hidden old events, first input, failure retry, and resume

## 4. Documentation and verification

- [x] 4.1 Update `AGENTS.md` and `docs/design.md` with deferred creation and blank-history semantics
- [x] 4.2 Run protocol sync check, bridge tests, Rust formatting/clippy checks, and full `cargo test`
