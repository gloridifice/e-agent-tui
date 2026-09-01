## 1. Provider-neutral prompt and clipboard model

- [x] 1.1 Add owned prompt text/image part and clipboard paste-result types to `e-tui`, and update agent requests, effect results, queues, and scripted ports to carry them.
- [x] 1.2 Extend `InputState` with atomic image blocks, ordered prompt extraction/restoration, cursor/deletion behavior, and focused model tests.
- [x] 1.3 Render `[Image <name>]` blocks with display-width-aware truncation and add TestBackend composer regression coverage.
- [x] 1.4 Route clipboard-image completion, mixed/image-only submission, queue dispatch, history drafts, and deferred-new restoration through the runtime controller.

## 2. Clipboard production adapters

- [x] 2.1 Implement Pi clipboard image-to-temporary-PNG handling with path-text fallback and focused filesystem/encoding tests.
- [x] 2.2 Implement DSH clipboard image-to-encoded-PNG handling with bounded generated names and text fallback.

## 3. DSH wire and Host admission

- [x] 3.1 Upgrade `bridge/protocol-contract.json` to v7 with typed ordered prompt parts on input/new-input and image payloads on command, then regenerate all derived protocol artifacts.
- [x] 3.2 Extend Rust DSH protocol DTOs and adapter conversion for ordered text/image prompts, Base64 encoding, image-only input, and preflight frame bounds.
- [x] 3.3 Add a focused Node `session-prompt` adapter over lazy `apiProxy.sessions.prompt` and route image-bearing input/new-input through Host admission with bounded errors and cross-connection guards.
- [x] 3.4 Pass command images to `commands.execute` without changing direct-command abort behavior, and reject unsupported image-command combinations without dropping the draft.

## 4. Verification and documentation

- [x] 4.1 Add focused Rust runtime/input/UI regressions for image block rendering, editing, queueing, image-only submit, and draft restoration.
- [x] 4.2 Add Node bridge and contract tests for mixed content, pure images, Host rejection/unavailability, new-session routing, and command images.
- [x] 4.3 Run focused Rust tests, bridge tests, protocol sync check, and formatting checks; fix all failures introduced by this change.
- [x] 4.4 Update the current client/bridge architecture documentation and user-visible help/README key reference only where the final interaction or boundary changed.
