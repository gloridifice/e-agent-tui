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
Known tool schemas SHALL have dedicated primary presentations. Read and view SHALL show a workspace-relative `path[:lines]`; command, bash, and pwsh SHALL show a Coral-equivalent `$`, command text using the shared command-token presentation, and a Bark-equivalent `lines x, duration y` metrics row; create SHALL show its path; filesystem search SHALL show a quoted query followed by `at "path"`. Preview naming and the transcript activity label SHALL preserve the original tool name for recognized shells (`bash`, `pwsh`, `cmd`, `powershell`, `sh`, `shell`), using `command` only as the fallback name for an unrecognized Command-capability tool.

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
Edit, replace, and insert calls SHALL use mutation content already supplied by their events. The client MUST NOT read the target file, compare before/after files, run an LCS/diff algorithm, or invent removed/context lines. Event-provided unified diff text SHALL retain every event-authored row in order; the client MAY classify those rows to present event-authored file paths, hunk coordinates, old/new line numbers, diff structure, and syntax-colored code bodies. Structured old/new fragments MAY be linearly rendered as removed and added rows. Diff syntax tokens SHALL use `semantics.markdown`, while added/removed backgrounds, accents, gutters, and separators SHALL use `semantics.diff`. When a pending mutation Preview contains requested fragments and its successful result later supplies an authoritative unified patch, the settled Preview SHALL replace the requested fragments with that patch on the same target.

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

#### Scenario: Event-provided unified diff is malformed
- **WHEN** a unified diff cannot be fully classified for line numbers or syntax selection
- **THEN** Preview retains all event-authored rows with fallback semantic styling and does not invent, reorder, or drop mutation content

### Requirement: Muted Markdown for injected context
Prompt-injection/context Preview SHALL use complete Markdown rendering through `semantics.markdown_weak`, whose role set matches normal Markdown while its bundled mappings are limited to Bark, Umber, and Night equivalents. It SHALL preserve Markdown and syntax-token bold, italic, and underline modifiers. Reasoning/thinking Preview behavior SHALL remain distinct in semantic content kind while using the same weak Markdown presentation context.

#### Scenario: Injected instructions contain Markdown
- **WHEN** an injected context message contains headings, bold, italic, code, links, or a language-labelled fenced code block
- **THEN** Preview renders the complete Markdown through weak semantic roles and preserves the corresponding modifiers and syntax distinctions

#### Scenario: Reasoning follows an injected context
- **WHEN** Preview later targets a reasoning/thinking block
- **THEN** it retains the reasoning semantic content kind and uses weak Markdown presentation rather than the injected-context content kind

#### Scenario: Normal transcript Markdown has the same source
- **WHEN** source rendered weakly for injected-context Preview also appears as transcript Markdown
- **THEN** the transcript rendering continues to use unchanged `semantics.markdown` roles

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

### Requirement: Long terminal output preserves tool information
A structured tool Preview with terminal secondary output SHALL wrap its tool-name and primary-information section to the Preview content width, but SHALL render each terminal-output source row as one display row clipped at the right edge without an added ellipsis. While the complete presentation fits, the combined content SHALL retain ordinary vertical centering. Once output growth would scroll the information section beyond the top edge, the information section SHALL remain pinned at the top and the remaining viewport rows SHALL show the newest terminal-output tail.

#### Scenario: Tool Preview fits in the pane
- **WHEN** the wrapped tool information and terminal output fit within the Preview height
- **THEN** the combined presentation remains vertically centered and no sticky positioning is applied

#### Scenario: Long output reaches the top edge
- **WHEN** terminal output grows until the bottom-anchored presentation would move the tool information above the Preview
- **THEN** the complete wrapped information section remains visible at the top and output occupies only the rows below it

#### Scenario: Output line exceeds the pane width
- **WHEN** one terminal-output source row is wider than the Preview content width
- **THEN** it occupies exactly one display row, is clipped at the right edge, and receives no synthetic ellipsis

#### Scenario: Wrapped information consumes the available height
- **WHEN** the tool information section alone is at least as tall as the Preview viewport
- **THEN** the viewport prioritizes the top of the information section and renders no terminal-output row over it

### Requirement: Shared command-token presentation
Structured tool commands and standalone command Preview SHALL use the same presentation rules as History command summaries. Each simple command's executable token SHALL use a Blush-equivalent foreground; arguments, subcommands, quote delimiters and quoted argument contents SHALL use Mist-equivalent. Unquoted `-`/`--`-prefixed flags SHALL use Mist-equivalent italic text; in `--key=value`, only `--key=` SHALL be italic. Standalone `--` SHALL end flag recognition until the next command. Unquoted command separators (`&&`, `||`, `;`, `|`, `|&`, `&`) and redirection operators (`>`, `>>`, `<`, including descriptor forms) SHALL use Bark-equivalent without italics. Separators SHALL begin a new executable position; redirection targets SHALL remain arguments, including a target before the executable. Quoted or escaped operators SHALL remain literal text. A quoted executable path SHALL retain executable styling.

Highlighting SHALL be presentation-only, use caller-supplied semantic foregrounds rather than fixed palette values, and add no background or border. It SHALL preserve command source and leave wrapping, clipping, copying, and export ownership with the consuming surface. Multiline commands SHALL preserve source line boundaries; an unquoted newline SHALL begin a new command unless it continues a chain or is escaped. Incomplete quotes SHALL remain displayable without losing source text. This presentation SHALL NOT require executing commands or fully interpreting shell grammar.

#### Scenario: Flags and arguments
- **WHEN** the command is `cargo run -p e-pi --theme=ferra`
- **THEN** `cargo` uses Blush-equivalent, `-p` and `--theme=` use italic Mist-equivalent, and the remaining text uses regular Mist-equivalent

#### Scenario: Chained commands and redirects
- **WHEN** the command is `cargo check&&echo ok >> build.log; cat < build.log | head -n 5`
- **THEN** `cargo`, `echo`, `cat`, and `head` use executable styling, chain/redirection operators use Bark-equivalent, and `build.log` remains an argument

#### Scenario: Literal operators and incomplete quotes
- **WHEN** quoted arguments contain `&&`, `;`, `|`, or `>>`, or input ends in an incomplete quote
- **THEN** their contents are retained as argument text without starting another command

#### Scenario: Multiline and narrow presentation
- **WHEN** a command contains source newlines or wraps at a narrow width
- **THEN** executable and flag styles remain attached to their source tokens across displayed rows and no added background or border changes the surrounding surface

