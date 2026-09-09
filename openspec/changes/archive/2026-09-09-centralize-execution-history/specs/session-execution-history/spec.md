## MODIFIED Requirements

### Requirement: Session-local execution files
For each materialized session observed through `pie` or `dshe`, the client SHALL record execution history exclusively in `<e-config>/cache/e-pi/history/<workspace-key>/<session-key>.jsonl` or `<e-config>/cache/e-dsh/history/<workspace-key>/<session-key>.jsonl` respectively. Workspace identity SHALL follow the backend-confirmed absolute session cwd using versioned lexical normalization without Git-root promotion, symlink resolution, or blanket case folding. Workspace and session keys SHALL be stable and path-safe. File headers SHALL retain and validate provider/session identity, original cwd, and normalized workspace identity; equivalent normalized cwd spellings SHALL resolve to the same trace. Resume SHALL append to the same file with a new run identity. Native backend session storage SHALL remain unchanged. The client SHALL NOT read, migrate, delete, or modify legacy project-local execution histories or ignore files. It SHALL NOT automatically delete old traces or fall back to project-local storage when the user configuration root is unavailable.

#### Scenario: Launch from a repository subdirectory
- **WHEN** the backend confirms that the session cwd is a repository subdirectory
- **THEN** the trace is stored in that subdirectory's distinct workspace bucket under the frontend's central history root, not in the project or a Git-root bucket

#### Scenario: Resume and switch
- **WHEN** a previously recorded session is resumed and later replaced with a different session
- **THEN** its existing central file is appended for the resumed run and subsequent operations use the replacement session's confirmed identity and cwd

#### Scenario: Deferred new conversation
- **WHEN** a client-only new-conversation draft has no materialized session
- **THEN** no trace is created for it and history commands do not expose the retained previous session's trace

#### Scenario: Legacy history exists
- **WHEN** an attached session has project-local execution history but no central trace
- **THEN** recording starts a new central trace without reading or importing the legacy file, recovering its metrics, or changing project files

### Requirement: Explicit persistence and query failures
Trace writes and queries SHALL run outside UI state locks, preserve event order, and remain bounded without silently dropping records. A start SHALL be queued for persistence when observed rather than held until completion; accepted writes SHALL be flushed on orderly shutdown and before a successful path/export snapshot response. Power-loss durability SHALL NOT be claimed. Readers SHALL distinguish a partial final record, malformed records, unsupported versions, and identity mismatches from valid complete history; readable valid records SHALL remain inspectable with a visible incompleteness diagnostic. Competing writers SHALL not interleave or corrupt a session file; an unavailable writer, queue overflow, or I/O failure SHALL visibly mark recording incomplete/unavailable while leaving normal agent interaction usable. Queries SHALL be scoped by session/request identity and a finite record watermark. No operation SHALL silently return partial clipboard content as a complete export.

#### Scenario: Read-only cwd or competing writer
- **WHEN** the session cwd is read-only but central storage is writable and no competing writer holds the session trace
- **THEN** recording succeeds without writing into cwd; a competing session writer still causes an explicit recording-unavailable failure

#### Scenario: Unavailable central root or competing writer
- **WHEN** a trace cannot be safely opened in the central history root or the user configuration root cannot be resolved
- **THEN** the client reports recording unavailable, continues the session, and does not fall back to another location

#### Scenario: Truncated final JSONL record
- **WHEN** a crash leaves valid complete records followed by an incomplete line
- **THEN** inspection exposes the valid records with a warning and further recording does not concatenate new JSON onto the partial line

#### Scenario: Delayed result after session replacement
- **WHEN** a history query finishes after the active session changes
- **THEN** it does not update the new page, insert the old path into its composer, or trigger a stale clipboard write

## ADDED Requirements

### Requirement: Recoverable workspace registry
Each frontend history root SHALL contain a versioned `workspaces.json` mapping workspace directory keys to normalized absolute workspace paths and readable original paths. Concurrent registration SHALL preserve unrelated mappings through bounded cross-process locking and atomic replacement. Event appends SHALL NOT rewrite this registry. Missing or malformed registries SHALL be recoverable from validated headers in the new history root only; malformed originals SHALL be preserved before replacement. Unsupported versions and identity conflicts SHALL be explicit failures rather than silently overwritten mappings. Moving a workspace SHALL create a distinct bucket rather than guessing an association with an old path.

#### Scenario: Concurrent workspace registration
- **WHEN** different clients register different workspaces in the same frontend history root
- **THEN** both mappings survive and their session records remain in separate buckets

#### Scenario: Missing or corrupt registry
- **WHEN** the registry is missing or malformed and valid central trace headers exist
- **THEN** their workspace mappings are recovered, any malformed registry is preserved, and existing session records remain intact

#### Scenario: Conflicting or unsupported metadata
- **WHEN** a registry or trace claims an incompatible version or a directory mapping inconsistent with its workspace identity
- **THEN** recording reports the conflict without overwriting that metadata or appending to the mismatched trace
