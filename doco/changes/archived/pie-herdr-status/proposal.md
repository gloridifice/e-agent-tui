<!-- doco:change mode=proposal-only -->
# pie-herdr-status

## Purpose

Expose the state of `pie` in its Herdr pane. Herdr's bundled Pi integration excludes RPC mode, so it cannot represent the runtime and user interactions hosted by `pie`.

## Scope and acceptance

- Add an adapter-owned Rust reporter in `e-pi`, using Herdr's CLI and the inherited pane identity only when `HERDR_ENV=1` and the binary, socket, and pane variables are present. No plugin installation or frontend configuration is required.
- Use the custom source `custom:pie` and agent label `pie`. Report initial unknown state until attachment, idle when ready, working while a run, command, or new-session submission is pending, and blocked while an approval, question, or native authentication interaction needs user input. Ordinary navigation pages do not block the agent. Messages are fixed labels, never prompt contents, credentials, authorization links, or device codes.
- Observe reduced runtime state without adding Herdr concepts to `e-tui`. Keep Pi's existing `agent_settled` completion semantics; do not infer completion from a tool result, turn end, or `agent_end`. Herdr owns unseen-result/done presentation.
- Use an asynchronous, single-writer worker with a latest-state channel, deduplication, increasing report sequence numbers, finite CLI timeouts and at most one retry per state. Herdr output stays off the terminal; delivery failure cannot fail the conversation or block the UI loop. No idle polling is introduced.
- Release the same source on normal exit and returned startup/runtime errors, after outstanding reports, with bounded CLI execution. Abrupt process termination cannot guarantee delivery.
- Do not report native session references, add automatic restore, modify Herdr, change the official Pi extension, or change DSH behavior.
- Update the current adapter contract, architecture ownership, and Pi usage documentation. Validate with a scoped build, lint, and existing lifecycle/architecture checks; do not add or modify tests.

## Verification

- `cargo check -p e-pi --bin pie` and `cargo build -p e-pi --bin pie` passed.
- Existing checks passed: `cargo test -p e-pi --test architecture` (2), `cargo test -p e-pi --lib adapter::queue_tests` (6), and `cargo test -p e-pi --lib extension_select_round_trips_through_question` (1). No tests were added or modified.
- `cargo clippy -p e-pi --bin pie --no-deps` completed with four pre-existing warnings in `main.rs` and no reporter warnings. The earlier warnings-as-errors invocation was blocked by existing `e-tui` lint findings; unrelated cleanup is deferred.
- An isolated Herdr 0.9.0 session running the built executable confirmed `pie`/idle on attachment, blocked during a native authentication prompt, idle after cancellation, and removal of the custom identity on exit. Missing-runtime startup also returned to an ordinary shell. The isolated session was stopped and the previous last-session state restored.
- A real model turn, approval/question dialogs, and Herdr outage recovery were not exercised end to end. Their status mapping and bounded worker behavior were inspected; no model credentials were added for validation.

## Result

Delivered. `e-pi` owns optional Herdr pane-local status reporting: it activates only in a
Herdr-managed environment with complete pane identity, uses the `custom:pie` source and `pie`
agent, and maps runtime and interaction state to unknown/idle/working/blocked with fixed labels.
Reporting runs in a bounded asynchronous single-writer worker with latest-state deduplication,
increasing sequence numbers, CLI timeouts, one retry, and source release on exit or returned
errors. `e-tui` gained no Herdr concepts, Pi's settled-run completion semantics are unchanged,
and no restore authority is claimed. The adapter contract, architecture ownership, and Pi usage
documentation were updated.

Acceptance evidence as recorded above: scoped build, lint, and existing checks passed, and an
isolated Herdr session confirmed attachment, blocked authentication, and source removal on exit.
Known limitations retained: a live model turn, approval/question dialogs, and Herdr outage
recovery were not exercised end to end, and abrupt process termination cannot guarantee release.
Completion and archiving were requested separately.
