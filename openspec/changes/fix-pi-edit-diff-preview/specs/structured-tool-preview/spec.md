## MODIFIED Requirements

### Requirement: Event-authored mutation Preview
Edit, replace, and insert calls SHALL use mutation content already supplied by their events. The client MUST NOT read the target file, compare before/after files, run an LCS/diff algorithm, or invent removed/context lines. Event-provided unified diff text SHALL be rendered verbatim; structured old/new fragments MAY be linearly rendered as removed and added rows. When a pending mutation Preview contains requested fragments and its successful result later supplies an authoritative unified patch, the settled Preview SHALL replace the requested fragments with that patch on the same target.

#### Scenario: DSH edit provides applied contextual hunks
- **WHEN** a completed edit result carries ordered `meta.diffs` hunks
- **THEN** Preview renders those DSH-computed hunks in order and replaces any less-specific pending call hunk

#### Scenario: str_replace_editor provides requested replacement
- **WHEN** `str_replace_editor` emits `command: "str_replace"` with `old_str` and `new_str` but no result-time applied hunk
- **THEN** Preview renders the event-provided requested replacement and retains it after settlement without claiming extra applied context

#### Scenario: Pi edit provides requested replacements
- **WHEN** Pi emits an `edit` call with a path and one or more `edits[]` entries containing `oldText` and `newText`
- **THEN** Preview renders the event-provided replacements as ordered removed/added fragments instead of generic JSON

#### Scenario: Pi edit result provides an authoritative patch
- **WHEN** a successful Pi edit result carries a standard unified patch in `details.patch`
- **THEN** the same `tool:<call-id>` Preview target renders that event-authored patch and replaces the pending requested fragments

#### Scenario: Pi replay uses a legacy single replacement
- **WHEN** a replayed Pi edit call carries top-level `oldText` and `newText` instead of `edits[]`
- **THEN** Preview treats it as one event-provided requested replacement

#### Scenario: Insert provides only inserted text
- **WHEN** an insert call supplies `new_str` and `insert_line` without a before-image
- **THEN** Preview renders an addition-only hunk anchored to that line and does not synthesize removed or surrounding lines

#### Scenario: Mutation payload is incomplete
- **WHEN** an edit, replace, or insert event lacks the data required by its safe mutation presentation
- **THEN** Preview falls back to its path or bounded generic tool presentation rather than calculating a diff

#### Scenario: Create remains common-format
- **WHEN** a create call includes complete new-file content
- **THEN** Preview renders `create` plus the path and does not switch to an all-added diff

### Requirement: Stable tool Preview correlation and replay
A tool call and its correlated result SHALL update one stable Preview target. Settlement SHALL preserve the primary content, add only the permitted secondary or applied mutation data, increment the Preview revision, and preserve scroll for unchanged target identity. Live, snapshot, backward-history, and cross-page result-before-call processing SHALL converge on equivalent settled Preview content, including event-authored unified mutation patches.

#### Scenario: Command result settles current Preview
- **WHEN** a command call is the current target and its result arrives
- **THEN** the same `tool:<call-id>` target refreshes with final metrics and output instead of becoming an unrelated plain-text target

#### Scenario: Result arrives before its call during history loading
- **WHEN** a bounded tool result with mutation hunks or a unified mutation patch is loaded before the matching call
- **THEN** its preview facts are staged and the later call constructs the same settled Preview as ordinary live ordering

#### Scenario: Same target revision changes
- **WHEN** a call-time Preview revision is replaced by its result-time revision for the same call ID
- **THEN** Preview scroll is preserved and stale deferred completions cannot overwrite the newer revision
