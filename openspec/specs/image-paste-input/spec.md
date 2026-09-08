# image-paste-input Specification

## Purpose
TBD - created by archiving change add-image-paste-input. Update Purpose after archive.
## Requirements
### Requirement: Clipboard image intake is provider-neutral
The frontend SHALL request clipboard content through a replaceable runtime port and SHALL consume either ordinary text or a normalized encoded image without importing Pi, DSH, filesystem, or system-clipboard implementations into `e-tui`.

#### Scenario: Pi clipboard image
- **WHEN** the Pi production clipboard port reads an image
- **THEN** it writes a collision-free temporary PNG and returns the resulting path as ordinary text for insertion at the cursor

#### Scenario: DSH clipboard image
- **WHEN** the DSH production clipboard port reads an image
- **THEN** it returns PNG bytes, `image/png`, and a bounded display name without writing a path into the composer

#### Scenario: Clipboard contains text only
- **WHEN** an application clipboard read has no supported image but contains text
- **THEN** the existing normalized text paste behavior is preserved

#### Scenario: Clipboard read fails
- **WHEN** neither supported image content nor text can be read
- **THEN** the frontend reports a bounded clipboard error and leaves the current editor unchanged

### Requirement: Composer images are atomic blocks
The ordinary composer SHALL render each pending image as one `[Image <name>]` block whose filename is display-width truncated when necessary. The display marker MUST NOT be submitted as model text, and image blocks MUST NOT be inserted into non-editing Input Pages.

#### Scenario: Render a long image name
- **WHEN** a pending image name exceeds the block's display budget
- **THEN** the block uses a Unicode-safe ellipsis and retains a useful filename suffix within the budget

#### Scenario: Move across an image
- **WHEN** Left or Right reaches an image block boundary
- **THEN** the cursor crosses the whole block in one operation and never enters its display label

#### Scenario: Delete an image
- **WHEN** Backspace or Delete targets an adjacent image block
- **THEN** the complete block and its attachment bytes are removed atomically

#### Scenario: Paste while a non-editing page is visible
- **WHEN** clipboard image completion arrives while an Input Page without an active text editor is visible
- **THEN** the image does not mutate the hidden composer draft

### Requirement: Prompt lifecycle preserves image content
Submission, queueing, history-draft restoration, and failed deferred materialization SHALL retain complete ordered text/image prompt content. A prompt containing at least one image SHALL be sendable without non-whitespace text.

#### Scenario: Submit mixed content
- **WHEN** the composer contains text around one or more image blocks
- **THEN** the emitted agent request carries ordered text and image parts without marker-label text

#### Scenario: Submit only an image
- **WHEN** the composer contains an image block and no ordinary text
- **THEN** submission emits an image-bearing prompt rather than treating the composer as empty

#### Scenario: Queue while running
- **WHEN** an image-bearing prompt is submitted while the agent is busy
- **THEN** the complete prompt enters the session queue and is dispatched unchanged when the agent becomes idle

#### Scenario: Unsupported built-in command
- **WHEN** a built-in command line has pending image blocks but the command does not accept images
- **THEN** execution is rejected visibly and the complete composer draft remains available

### Requirement: DSH images use durable Host admission
The DSH adapter and bridge SHALL send encoded image prompt parts through `apiProxy.sessions.prompt`. The bridge MUST NOT insert encoded images directly into durable user messages, manufacture attachment references, or interpret a client path as an attachment reference.

#### Scenario: Admit a mixed DSH prompt
- **WHEN** the bridge receives valid ordered text/image content for an attached session
- **THEN** it calls `sessions.prompt` with `mode: "queue"`, the current session ID, and the same content order

#### Scenario: Host rejects an image
- **WHEN** DSH rejects the image MIME, bytes, dimensions, count, or route
- **THEN** the bridge returns a bounded error and does not retry through direct user-message injection

#### Scenario: Host API is unavailable
- **WHEN** image input arrives while `apiProxy.sessions.prompt` is unavailable
- **THEN** the bridge reports an explicit image-input failure and does not discard or downgrade the image silently

#### Scenario: Integrated command accepts images
- **WHEN** an image-bearing integrated command is executed
- **THEN** the bridge passes its encoded images through the command service image parameter under the existing abort and cross-session guards

### Requirement: Image payloads remain bounded
Image paste and transmission SHALL preserve the canonical frame-size limit and DSH Host attachment limits. Client-side checks MAY reject an obviously oversized encoded request, but Host admission SHALL remain authoritative for image validity.

#### Scenario: Encoded frame is too large
- **WHEN** a DSH image prompt would exceed the canonical client frame bound
- **THEN** the adapter rejects it before transport with a visible bounded error

#### Scenario: Multiple pasted images
- **WHEN** repeated image pastes exceed a Host image count or aggregate-byte limit
- **THEN** the Host rejection is surfaced without partially admitting the prompt through a separate bridge storage path

