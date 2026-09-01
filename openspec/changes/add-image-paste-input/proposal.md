## Why

The composer currently accepts clipboard text only, so users cannot paste an image and ask either supported agent runtime to inspect it. Pi and DSH expose different native image paths, requiring one provider-neutral composer experience with adapter-owned delivery.

## What Changes

- Add clipboard image intake alongside the existing text clipboard fallback.
- Render DSH-bound images as atomic `[Image <name>]` composer blocks with display-width-aware filename truncation, cursor skipping, and whole-block deletion.
- Keep Pi image paste compatible with the official Pi TUI by writing a temporary PNG and inserting its path as ordinary editable text.
- Allow mixed text/image and image-only prompt submission, queueing, draft restoration, and first-prompt materialization.
- Extend the DSH wire protocol with ordered text/image prompt parts and route uploaded image bytes through the injected `apiProxy.sessions.prompt` attachment-admission path.
- Pass images to image-capable integrated DSH commands or reject unsupported combinations without silently dropping attachments.

## Capabilities

### New Capabilities
- `image-paste-input`: Clipboard image intake, atomic composer image blocks, provider-specific delivery, and image-aware prompt lifecycle behavior.

### Modified Capabilities
- `canonical-wire-schema`: Describe and validate ordered text/image content on DSH input, new-input, and command messages.
- `deferred-new-conversation`: Materialize a draft from a complete mixed-content or image-only first prompt rather than text alone.

## Impact

Affected areas include the provider-neutral `e-tui` input/action/runtime-port model and composer rendering, the `e-pi` and `e-dsh` clipboard adapters, Pi temporary-file handling, DSH Rust wire DTOs and adapter conversion, the Node bridge dispatcher and a new prompt adapter, the canonical protocol version and generated artifacts, help/key documentation where applicable, and focused Rust/Node UI and protocol tests. Clipboard image encoding may require an image codec dependency in the executable adapters; DSH image bytes remain subject to the host attachment service and canonical frame-size limits.
