# structured-tool-preview Specification

## Purpose
TBD - created by archiving change refactor-tool-call-preview. Update Purpose after archive.
## Requirements
### Requirement: Common structured tool Preview layout
Every non-diff tool Preview SHALL render one tool-name header using the theme's activity-label semantic role, followed on the next row by one typed primary section with no intervening blank row. It SHALL render a blank row and secondary section only when secondary content exists. The presentation model MUST carry semantic content rather than terminal-width rows or hard-coded palette colors.

#### Scenario: Tool has only primary content
- **WHEN** a read, view, create, search, or generic tool Preview has no secondary content
- **THEN** the pane renders the Umber-equivalent tool-name header and the primary content directly beneath it without an intervening blank row or trailing secondary section

#### Scenario: Tool has command output
- **WHEN** a command Preview has terminal output
- **THEN** one blank row separates the primary command/metrics section from the secondary output

#### Scenario: Custom theme is active
- **WHEN** a theme maps activity label, primary text, muted detail, and prompt accent to colors other than Ferra's Umber, Mist, Bark, and Coral
- **THEN** the tool Preview uses those semantic mappings rather than hard-coded RGB values

### Requirement: Known tool primary presentations
Known tool schemas SHALL have dedicated primary presentations. Read and view SHALL show a workspace-relative `path[:lines]`; command, bash, and pwsh SHALL show a Coral-equivalent `$`, Mist-equivalent command text, and a Bark-equivalent `lines x, duration y` metrics row; create SHALL show its path; filesystem search SHALL show a quoted query followed by `at "path"`. Preview naming and the transcript activity label SHALL preserve the original tool name for recognized shells (`bash`, `pwsh`, `cmd`, `powershell`, `sh`, `shell`), using `command` only as the fallback name for an unrecognized Command-capability tool.

#### Scenario: Read has a closed line window
- **WHEN** a read or view call identifies `src/main.rs` and lines 12 through 20
- **THEN** its Preview header is `read` or `view` and its primary location is `src/main.rs:12-20`

#### Scenario: Command settles with bounded output
- **WHEN** a bash call settles after 1250 milliseconds with 14 retained output lines
- **THEN** its Preview header is `bash`, its primary shows `$ <command>` and `lines 14, duration 1.2s`, and its bounded output is the secondary section

#### Scenario: Command output was trimmed
- **WHEN** the bridge marks command output as a retained tail rather than the complete output
- **THEN** Preview qualifies the line count, such as `lines 14+`, and does not claim it is complete

#### Scenario: PowerShell tool name is normalized only for Preview
- **WHEN** DSH emits a `pwsh` tool call
- **THEN** the Preview header and transcript label are `pwsh` while existing transcript projection behavior remains unchanged

#### Scenario: Filesystem search identifies query and root
- **WHEN** a search call contains pattern `PreviewContent` and path `crates/e-tui`
- **THEN** its primary renders `"PreviewContent"` and a following `at "crates/e-tui"` row

### Requirement: Bounded generic JSON fallback
A tool call without a supported preview schema SHALL render its original tool name and a stable, bounded, pretty-printed JSON primary. Invalid JSON SHALL degrade to bounded raw argument text, and neither form SHALL create an automatic secondary result body.

#### Scenario: Unknown tool has structured arguments
- **WHEN** an unknown tool call contains valid nested JSON arguments
- **THEN** Preview renders the original tool name followed by indented JSON without interpreting DSH-specific keys

#### Scenario: Unknown arguments exceed the budget
- **WHEN** pretty-printed arguments exceed the generic Preview budget
- **THEN** the stored source is truncated at a valid character boundary and carries a visible truncation indication

#### Scenario: Unknown arguments are invalid JSON
- **WHEN** the DSH arguments string cannot be parsed as JSON
- **THEN** Preview renders a bounded raw fallback rather than failing event reduction

### Requirement: Safe two-tone ANSI command output
Command secondary output SHALL be parsed as terminal text without forwarding control sequences. Text under an explicit ANSI foreground color SHALL use the theme's Bark-equivalent tone, uncolored text SHALL use the Umber-equivalent tone, and bold and italic SGR effects SHALL be preserved. All other terminal effects and controls MUST be stripped or ignored.

#### Scenario: Colored and uncolored runs coexist
- **WHEN** output contains an uncolored prefix, a red SGR run, a reset, and an uncolored suffix
- **THEN** the prefix and suffix render Umber-equivalent, the red run renders Bark-equivalent, and no escape byte reaches the terminal backend

#### Scenario: Bold italic colored run
- **WHEN** output enables bold, italic, and an ANSI foreground color for one run
- **THEN** the run renders Bark-equivalent with both bold and italic modifiers

#### Scenario: Hostile control sequence is present
- **WHEN** output contains OSC title/hyperlink data, cursor movement, erasure, an unsupported CSI sequence, or a truncated escape
- **THEN** Preview emits no corresponding terminal control and remains bounded and renderable

### Requirement: Event-authored mutation Preview
Edit, replace, and insert calls SHALL use mutation content already supplied by their events. The client MUST NOT read the target file, compare before/after files, run an LCS/diff algorithm, or invent removed/context lines. Event-provided unified diff text SHALL be rendered verbatim; structured old/new fragments MAY be linearly rendered as removed and added rows.

#### Scenario: DSH edit provides applied contextual hunks
- **WHEN** a completed edit result carries ordered `meta.diffs` hunks
- **THEN** Preview renders those DSH-computed hunks in order and replaces any less-specific pending call hunk

#### Scenario: str_replace_editor provides requested replacement
- **WHEN** `str_replace_editor` emits `command: "str_replace"` with `old_str` and `new_str` but no result-time applied hunk
- **THEN** Preview renders the event-provided requested replacement and retains it after settlement without claiming extra applied context

#### Scenario: Insert provides only inserted text
- **WHEN** an insert call supplies `new_str` and `insert_line` without a before-image
- **THEN** Preview renders an addition-only hunk anchored to that line and does not synthesize removed or surrounding lines

#### Scenario: Mutation payload is incomplete
- **WHEN** an edit, replace, or insert event lacks the data required by its safe mutation presentation
- **THEN** Preview falls back to its path or bounded generic tool presentation rather than calculating a diff

#### Scenario: Create remains common-format
- **WHEN** a create call includes complete new-file content
- **THEN** Preview renders `create` plus the path and does not switch to an all-added diff

### Requirement: Muted Markdown for injected context
Prompt-injection/context Preview SHALL use complete Markdown rendering with every foreground mapped to the theme's muted-text tone while preserving Markdown modifiers. Reasoning/thinking Preview behavior SHALL remain unchanged and SHALL retain its distinct semantic content kind.

#### Scenario: Injected instructions contain Markdown
- **WHEN** an injected context message contains headings, bold, italic, code, or links
- **THEN** Preview renders the complete Markdown in the muted tone while preserving the corresponding modifiers

#### Scenario: Reasoning follows an injected context
- **WHEN** Preview later targets a reasoning/thinking block
- **THEN** it uses the existing reasoning path rather than the injected-context content kind

### Requirement: Stable tool Preview correlation and replay
A tool call and its correlated result SHALL update one stable Preview target. Settlement SHALL preserve the primary content, add only the permitted secondary or applied mutation data, increment the Preview revision, and preserve scroll for unchanged target identity. Live, snapshot, backward-history, and cross-page result-before-call processing SHALL converge on equivalent settled Preview content.

#### Scenario: Command result settles current Preview
- **WHEN** a command call is the current target and its result arrives
- **THEN** the same `tool:<call-id>` target refreshes with final metrics and output instead of becoming an unrelated plain-text target

#### Scenario: Result arrives before its call during history loading
- **WHEN** a bounded tool result is loaded before the matching call
- **THEN** its preview facts are staged and the later call constructs the same settled Preview as ordinary live ordering

#### Scenario: Same target revision changes
- **WHEN** a call-time Preview revision is replaced by its result-time revision for the same call ID
- **THEN** Preview scroll is preserved and stale deferred completions cannot overwrite the newer revision

