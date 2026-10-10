# Implementation

## Baseline and goals
Composer submission and the shared controller own admission. TimelineModel owns
canonical transcript composites, retained-source Preview and Reading. Adapters own
UiActionPorts and Tokio select loops. The goal is the approved !-command workflow;
no bridge or Pi RPC extension is needed.

## Overall approach
Return owned start/cancel effects and reduce background completions. Use an
existing activity/detail composite with a terminal card role, retained copy source
and inline terminal Preview. Process work stays in each independent adapter.

## APIs and data model
ShellRequest carries a stable DisplayId, command and optional workspace.
ShellResult carries bounded output, truncation, exit code, duration, cancellation
and error.
ShellState owns one active identity, a monotonic counter surviving transcript
resets, and draft-local/unfolded identity references (not a second transcript store).
UiActionPorts starts/cancels adapter tasks; runners select their completions.

## Algorithms and rules
Route leading ! before ordinary prompt admission. Preserve expanded source and
prompt history; empty/image-bearing commands fail locally with draft restore.
Use PowerShell on Windows (prefer pwsh, fallback to Windows PowerShell), the user's
shell or sh on Unix. Null stdin; capture stdout/stderr into one temporary file
using shared handles to retain write order. Read only a bounded file tail, discard
any partial leading line, decode UTF-8 lossily and enforce byte/line retention. Background completion and process-tree interruption remain outside
UI guards. Stable ownership rejects stale results; exit/session clearing cleans up.
Running composites reserve one message-viewport row and are excluded from the
scrollable row cache to avoid duplication. On completion move the canonical node
to the transcript tail and mark it unfolded; display its last 64 wrapped rows.
User submissions and live TurnEnd clear unfolded identities. Reading overrides
folding only for its focused composite. Focus changes invalidate presentation
geometry, never retained source. Raw
terminal output uses safe ANSI parsing, not Markdown. Draft-local IDs reference
canonical nodes; prior-session nodes remain hidden and no native session is
materialized by shell submission. Local output is not persisted in agent history.

## Fixed decisions and discretion
One local command runs at a time; unrelated model work stays independent. No PTY,
interactive programs, live output streaming, or native history changes. Existing
Reading/Preview/copy actions and shared Unicode wrapping remain authoritative.
Local helper names and equivalent rendering details are discretionary.
Blocked: none.

## Follow-up retention and presentation
The user-approved follow-up replaces the original five-row/full-retention policy
with three presentation states and a bounded cache. Retention constants live in
`e-tui::shell`; both adapters bound file-tail reads before returning results.
Current contracts define the delivered behavior. Temporary disk capture is still
released on command completion; streaming and PTYs remain out of scope.

## Verification and documentation impact
Do not add or modify tests. Run scoped existing composer/controller/Reading/Preview
tests, workspace formatting/Clippy and adapter builds. Exercise successful, failed,
long and cancelled commands in a real terminal. Synchronize current presentation,
interaction, runtime and architecture contracts; document the noninteractive
workflow in README and localized help. Do not complete or archive this change.
