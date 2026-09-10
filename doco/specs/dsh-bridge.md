# DSH bridge contracts

## Composition and lifecycle

- `bridge/` is the sole production Node.js source. `index.js` owns composition/socket wiring; focused modules own dispatch, connection, history, session, prompt, model, command, question, skill, trimming, and frame behavior.
- All host side effects MUST use DSH effect ownership. Detach MUST remove listeners, abort work, revoke approvals, and close or retain the socket according to the requested transition.
- Any handler using a connection after `await` MUST verify that the captured connection is still current and registered.
- Async outcomes MUST retain session/connection identity and MUST NOT leak into a replacement session.

## Framing and history

- `bridge/protocol-contract.json` is the only hand-written source for DSH protocol version, limits, rosters, and payload shapes.
- Every outgoing frame MUST pass bounded encoding. Snapshot/history pages keep the newest fitting suffix and report truncation; a singular oversized frame becomes a bounded `frame-too-large` error.
- Active history reads in-memory events. Cold history may read persistence, but stale/aborted reads MUST NOT update or send.
- Read tool results are stripped. Other tool output and rich metadata are tail-bounded. Unknown events with surface metadata retain only bounded identity/surface fields.

## Session and prompt routing

- Create and cold resume MUST install model selection and the recorded preset before attachment. New sessions MUST claim the workspace resolved from the chosen cwd.
- Cwd priority is validated client cwd, then current session header cwd, then process cwd. Reused connections retain client cwd.
- Text/image prompt order MUST be preserved. Image admission uses the host session prompt API; rejection MUST NOT synthesize durable attachments or inject encoded bytes through text queues.
- ASAP admission and clear are serialized per connection and acknowledge the latest authoritative queue snapshot.

## Commands and host services

- Effective commands, skills, models, questions, and login data come through their owning host services. Missing optional services fail through bounded user-visible errors rather than hanging.
- Direct command executions own abort controllers and return typed results. Interrupt aborts active commands and the agent turn.
- Question relay MUST acquire optional API proxy services lazily and clean up across service reloads.
- Model selection MUST mutate local state only after host confirmation and MUST preserve submission order with dependent prompts.

## Deployment

- `tools/sync-release-assets.mjs` mirrors production bridge files into `crates/e-dsh/assets/bridge`. Checkout and packaged builds MUST consume equivalent inputs, and check mode MUST reject missing, stale, or extra files.
- Compatibility-sensitive bridge changes require deployment, DSH restart, and the deployed-copy verification gate; source-only tests are insufficient.
