## MODIFIED Requirements

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

## ADDED Requirements

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
