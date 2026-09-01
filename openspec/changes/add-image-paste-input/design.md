## Context

`e-tui` currently owns a plain UTF-8 composer plus atomic large-text paste ranges, while `UiActionPorts::read_clipboard` returns text only and `AgentRequest::{Input,NewInput}` carry a single string. Pi's official interactive client handles clipboard images by writing a temporary image and inserting its path. DSH instead exposes encoded prompt image parts through `apiProxy.sessions.prompt`; that API validates bytes and promotes them to durable attachment references. The existing bridge directly constructs text-only user messages, so encoded images cannot safely enter that path.

The frontend must remain provider-neutral, preserve the current text paste path, and coexist with uncommitted runtime-boundary work already present in the workspace.

## Goals / Non-Goals

**Goals:**

- Paste a clipboard image into the active ordinary composer and render a compact atomic image block for attachment-capable adapters.
- Keep Pi behavior aligned with upstream by inserting a generated temporary PNG path as ordinary text.
- Represent complete prompt content, including image-only input, in owned provider-neutral actions, queues, history drafts, and deferred new-conversation state.
- Admit DSH image bytes only through the Host API and durable attachment service.
- Bound display labels and wire payloads and make failures visible without silently losing attachments.

**Non-Goals:**

- Rendering image pixels in the terminal.
- Adding Pi RPC `images` support in this change.
- Treating local filesystem paths as DSH attachment references.
- Implementing arbitrary non-image files or drag-and-drop.
- Reimplementing DSH image normalization, storage, or route capability checks in the bridge.

## Decisions

1. **Clipboard results are adapter-owned but normalized.** `UiActionPorts` returns a provider-neutral paste result: ordinary text or an encoded image with MIME type, display name, and bytes. The Pi port writes clipboard RGBA data as PNG in a dedicated temporary directory and returns the path as text. The DSH port encodes the same RGBA data as PNG and returns an image. This keeps filesystem policy out of `e-tui` while allowing one controller completion path. Text remains the fallback when no image is available.

2. **Images are atomic composer objects, not marker text.** `InputState` retains image metadata beside an atomic raw-buffer range and projects that range as `[Image <name>]`. The filename is truncated by terminal display width with a single ellipsis while retaining a useful extension suffix. Left/right movement skips the complete block; Backspace/Delete and word deletion remove the block and metadata together. The display marker never becomes model text.

3. **Prompt content is an ordered provider-neutral value.** A prompt owns `Vec<PromptPart>`, where each part is text or an image. Submission scans composer text and image ranges in order, coalesces adjacent text, and allows at least one non-empty image even when text is empty. `AgentRequest::Input`, `NewInput`, and image-capable `Command` carry complete owned prompt content. Queue and draft restoration retain this value rather than flattening it to a string.

4. **Pi receives path text only.** The Pi clipboard port returns the generated path as text, so Pi requests remain text-only and no RPC schema changes are required. Files use collision-free `pi-clipboard-<uuid>.png` names under a process-independent temporary location; failed writes return an ordinary clipboard error. This mirrors upstream Pi and avoids inventing a second Pi attachment lifecycle.

5. **DSH wire content mirrors Host prompt parts.** Protocol v7 adds `PromptTextPart` and `PromptImagePart` records and ordered `content` fields on `input` and `new-input`; command frames carry encoded image entries beside the command line. Rust serializes bytes as canonical Base64 only at the DSH boundary. The bridge validates the shallow shape and frame bound but leaves byte/MIME/dimension admission authoritative to DSH.

6. **The bridge uses `apiProxy.sessions.prompt` for ordinary image prompts.** A focused `session-prompt.js` adapter lazily resolves `apiProxy.sessions`, mints an RPC id, calls `session.prompt` with `mode: 'queue'`, and unwraps the RpcResult. It is used whenever an input contains images; text-only input may retain the direct `followup` path to minimize compatibility risk. The bridge never places encoded image objects directly in a durable `UserMessage` and never manufactures attachment references. Cross-await connection identity is checked before reporting results.

7. **Commands never discard images.** Integrated DSH commands receive encoded images through the existing `commands.execute(agent, line, images, signal)` parameter. A built-in command that cannot meaningfully own attachments is rejected before mutating composer state, leaving the draft intact. Pi path paste is ordinary command text and needs no special command attachment behavior.

8. **Current paste shortcut remains the entry point.** Application-owned clipboard read uses the existing `Ctrl+V` fallback and attempts the adapter's image behavior before text fallback. Bracketed terminal paste remains text-only. No new shortcut is required; help text describes that `Ctrl+V` accepts text or an image where supported.

## Risks / Trade-offs

- **[Base64 expansion can exceed the WebSocket frame bound]** → Preflight encoded size in the Rust DSH adapter, retain the canonical frame-size enforcement, and surface host admission failures.
- **[Clipboard APIs often provide no source filename]** → Generate a stable short `clipboard-<id>.png` display name; never expose a local path in a DSH image name.
- **[Atomic object ranges complicate editing and history]** → Reuse the established paste-block boundary discipline and add focused cursor/deletion/history/UI tests.
- **[A DSH API service may be unavailable during plugin reload]** → Resolve lazily per send and return an explicit bounded error without falling back to unsafe direct injection.
- **[Creating a deferred session can succeed before image admission fails]** → Keep the client draft until a successful welcome/turn transition and leave blank-session filtering authoritative; report the admission error and permit retry.
- **[Temporary Pi images can accumulate]** → Use the OS temporary directory and upstream-compatible names; lifecycle cleanup is intentionally left to OS/user temp maintenance rather than deleting files that resumed sessions may still reference.

## Migration Plan

1. Introduce normalized prompt/image and clipboard result types while keeping text-only constructors convenient.
2. Add atomic image composer projection and tests.
3. Implement Pi and DSH clipboard production ports.
4. Upgrade the DSH contract and both wire implementations, regenerate derived artifacts, and add bridge prompt admission.
5. Add image-aware queue/new-draft/command behavior and regression tests.
6. Update current architecture/help documentation and run focused Rust, bridge, and protocol checks.

Rollback requires deploying the matching v6 bridge/client pair because protocol v7 peers intentionally reject a version mismatch. Pi remains independently rollback-safe because its RPC contract is unchanged.

## Open Questions

None. The initial release accepts one clipboard image per paste action and naturally supports multiple blocks through repeated paste actions, subject to DSH host limits.
