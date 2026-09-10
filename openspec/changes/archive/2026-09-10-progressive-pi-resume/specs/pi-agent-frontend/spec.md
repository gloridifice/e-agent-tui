## MODIFIED Requirements

### Requirement: Native session resume roster
`pie` SHALL display current-project native Pi sessions in the existing Resume page, asynchronously and newest file modification first. It MUST treat session files as read-only and MUST use Pi RPC for switching or creating sessions. Candidate enumeration MAY inspect all file metadata to establish ordering, but content loading SHALL be demand-driven in batches of at most twice the rendered list body height. Loaded batches SHALL become visible without waiting for all candidates; browsing near the loaded boundary SHALL request another batch. There SHALL be no fixed file-count cutoff hiding older candidates. Search SHALL progressively examine unloaded candidates and distinguish an unfinished search from a completed empty result. Page closure, reopening, and workspace changes MUST isolate stale results without moving the active selection or reopening the page.

Title discovery SHALL use bounded reads, preferring the last discoverable native name, then a first-user-message fallback, then a default title. Valid matching headers MUST remain selectable when file size, record count, malformed later records, or metadata scan budgets prevent complete title discovery. Pi rows SHALL report local file modification time as `YYYY-MM-DD HH:mm`.

#### Scenario: List current-project sessions
- **WHEN** the user opens `/resume`
- **THEN** the page stays responsive while native files are enumerated and the newest matching sessions are loaded first, in batches capped at twice the rendered list body height, without modifying files

#### Scenario: Browse older sessions
- **WHEN** the user approaches the end of loaded rows and older candidates remain
- **THEN** another bounded batch is loaded asynchronously and appended while preserving the selected session and viewport

#### Scenario: Search beyond loaded rows
- **WHEN** the user enters a title or identity search that matches an unloaded old session
- **THEN** background batches continue across unloaded candidates, matching rows appear progressively, and an empty result is final only when the scan finishes

#### Scenario: Large or partly damaged session
- **WHEN** a file has a valid current-project header but later metadata exceeds the reading budget or contains malformed records
- **THEN** the session remains selectable with the best bounded title or default title rather than being hidden

#### Scenario: Session file is malformed
- **WHEN** a candidate native session file has an invalid or unreadable header
- **THEN** `pie` skips that candidate, emits a bounded diagnostic, and continues listing valid sessions

#### Scenario: Obsolete completion
- **WHEN** a batch completes after page closure, page reopening, or a workspace change
- **THEN** it cannot populate the replacement page, change its selection, or reopen the closed page

#### Scenario: Select a session
- **WHEN** the user selects a Resume row
- **THEN** `pie` sends the corresponding native session path to Pi `switch_session` and rebuilds the frontend from authoritative queries
