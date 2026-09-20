<!-- doco:change mode=proposal-only -->
# pie-resume-command

## Purpose

Allow users to reopen a Pi session by its native session ID and leave a usable recovery command in the terminal after closing `pie`.

## Scope and acceptance

- Add `pie --resume <session_id>` and `pie -r <session_id>`. Require a non-empty ID and reject conflicting session selectors. Preserve existing `--session <file>` and positional-file launches.
- Resolve the exact ID from native session storage, including the existing session-directory environment overrides. Read bounded headers outside frontend locks; reject missing or ambiguous IDs before terminal setup. Launch Pi with the resolved file and its saved workspace, without an interactive picker or cross-project fork prompt.
- After restoring the terminal, print a recovery command for the last attached, saved session, including after an in-TUI session change. Do not print a recovery command for an unsaved session or failed startup. Use the ID for sessions in native storage and a quoted `--session` file command for files outside that storage.
- Keep discovery and process effects in `e-pi`; do not change the native session format, shared frontend contracts, or Pi itself. No prefix matching, new session picker, or automatic session saving is included.
- Update CLI help, current session contracts, and user-facing launch guidance. Validate with scoped existing Rust tests, a build, and CLI/manual smoke checks; do not add or modify tests.

## Result

Implementation and scoped verification are delivered; lifecycle completion and archive were not requested.

Verification:
- `cargo build -p e-pi --bin pie` passed.
- Existing CLI, session-index, and process tests passed (11 tests); no tests were added or modified.
- CLI/PTY smoke checks verified both resume flags, saved file/workspace forwarding, missing and ambiguous IDs, and read-only discovery.
- A real TUI driven through `tui-test` with a temporary RPC stub verified the recovery command after switching sessions and suppressed recovery output for an unsaved session.
- CLI help, missing arguments, and conflicting selectors were checked manually.
