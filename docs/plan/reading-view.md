# Reading View and Preview pane

## Purpose

Reading View replaces the existing row-oriented Copy Mode. It treats the transcript as a semantic document made of Blocks and Items. The same Preview pane is present in normal mode and Reading View, but each mode chooses its target differently.

Normal mode:

```text
newest Block -> Preview pane
```

Reading View:

```text
current Item, when present -> Preview pane
otherwise current Block    -> Preview pane
```

The Preview pane is one Region with one state model, one resolver contract, and one cache. There is no separate Content pane.

## Terminology

### Reading View

The interaction mode entered with the selected Reading View binding. `Ctrl+V` is the default candidate; if the compatibility gate rejects it, one documented alternate is used consistently. The mode enables a Block cursor, optional Item cursor, semantic copying, and cursor-driven Preview selection.

### Block

A `ReadingBlock` is a stable semantic navigation and copy unit in the transcript. A Block may occupy several rendered rows. Rewrapping must not change its identity.

### Item

A `ReadingItem` is a navigable semantic target inside one Block. Examples include a Markdown link and a file path inside a tool activity.

### Preview

`PreviewContent` is the detailed representation shown in the Preview pane. It may be stored inline or resolved asynchronously by the active kernel adapter.

## Semantic model

```rust
pub struct ReadingDocument {
    pub blocks: Vec<ReadingBlock>,
}

pub struct ReadingBlock {
    pub id: BlockId,
    pub kind: BlockKind,
    pub copy: CopyPayload,
    pub preview: Option<PreviewRef>,
    pub items: Vec<ReadingItem>,
}

pub struct ReadingItem {
    pub id: ItemId,
    pub kind: ItemKind,
    pub preview: PreviewRef,
}
```

An Item belongs to exactly one Block. The document stores Blocks in transcript order.

## Block taxonomy

```rust
pub enum BlockKind {
    TextParagraph,
    CodeBlock,
    ListItem,
    Mermaid,
    ToolActivity(ToolCapability),
    Reasoning,
    Custom(CustomBlockKind),
}
```

### Markdown blocks

| Markdown source | Block rule |
| --- | --- |
| Paragraph | One paragraph is one Block, even when it wraps to several display rows. |
| Fenced code | The complete code fence is one Block. |
| List | Each semantic list row is one Block. Wrapped continuation rows remain in that Block. |
| Mermaid | One rendered diagram is one Block. |
| Link | The containing Markdown Block owns a link Item. |

Tables should keep the existing atomic source identity. Their initial Reading View treatment can use `Custom` until a table-specific navigation model is defined.

### Tool blocks

One tool lifecycle is one Block. Running, settled, and enriched forms keep the same `BlockId`.

A tool Block may contain file Items or other adapter-provided Items. The adapter maps raw tools to `ToolCapability` before `e-tui` receives them.

### Reasoning blocks

One continuous reasoning segment is one Block. A reasoning Block is eligible for navigation only when the current presentation mode renders it. Hidden reasoning must not create an invisible cursor target.

### Plain and unknown content

User plain text is split into text paragraph Blocks. Unknown fallback content may become a `Custom` Block when it has a stable identity, at least one rendered row, and a complete copy payload.

## Block identity

A Block ID represents semantic ownership, not a display position.

```text
80-column layout:
  Block A -> rows 10 through 14

120-column layout:
  Block A -> rows 10 through 12

BlockId remains A
```

The following operations must preserve IDs where the semantic unit is unchanged:

- terminal resize;
- page width change;
- streaming assistant growth;
- tool settlement;
- animation patch;
- history prepend;
- Preview resolution;
- theme change and layout rematerialization.

Surface replacement may remove old Block IDs and insert a replacement Block according to the existing surface ownership rules.

## Layout model

`ReadingDocument` is width-independent. `ReadingLayout` maps semantic IDs to rendered geometry for the current transcript width.

```rust
pub struct ReadingLayout {
    pub blocks: HashMap<BlockId, BlockLayout>,
    pub items: HashMap<ItemId, ItemLayout>,
}

pub struct BlockLayout {
    pub rows: Range<usize>,
    pub rail: Rect,
}

pub struct ItemLayout {
    pub fragments: Vec<Rect>,
}
```

An Item may have several fragments. A long link, for example, can wrap across two terminal rows. Spatial navigation considers all fragments but keeps one semantic Item cursor.

The layout is generated from the same width-aware transcript representation used by rendering and copy provenance. It must not introduce a second wrapping implementation.

## Preview model

### Preview values

```rust
pub enum PreviewContent {
    Link {
        label: Option<String>,
        url: String,
    },
    Diff {
        path: String,
        unified: String,
    },
    Lines {
        path: Option<String>,
        text: String,
        range: Option<LineRange>,
    },
    SearchResult {
        query: String,
        result: String,
    },
    Command {
        command: String,
        output: String,
        exit_code: Option<i32>,
    },
    Path {
        path: String,
    },
    Markdown(String),
    PlainText(String),
    Custom(CustomPreview),
}
```

### Inline and deferred values

```rust
pub enum PreviewRef {
    Inline(PreviewContent),
    Deferred {
        key: PreviewKey,
        revision: u64,
    },
}
```

A complete event may carry an inline Preview. An adapter may use a deferred key when it must read a file, calculate a diff, or ask the kernel for more data.

### Pane state

```rust
pub enum PreviewPolicy {
    FollowLatestBlock,
    FollowReadingCursor,
}

pub enum PreviewTarget {
    Block(BlockId),
    Item {
        block_id: BlockId,
        item_id: ItemId,
    },
}

pub enum PreviewState {
    Empty,
    Loading {
        key: PreviewKey,
        request_id: PreviewRequestId,
    },
    Ready(PreviewContent),
    Error(String),
}

pub struct PreviewPaneState {
    pub policy: PreviewPolicy,
    pub target: Option<PreviewTarget>,
    pub state: PreviewState,
    pub scroll: usize,
}
```

`PreviewPaneState` does not store a second copy of transcript text or tool state. It stores selection and resolved presentation data.

## Preview mapping

Adapters and projections provide standard Preview values for known capabilities:

| Block or Item | Preview target | Preview value |
| --- | --- | --- |
| Markdown link | Item | Full link URL and optional label |
| Edit file | Item | Diff |
| View file | Item | File lines |
| Read file | Item | File lines |
| Replace file | Item | Diff |
| Search or grep | Block | Full search result |
| Command | Block | Full command above full result |
| Create file | Block | File path |
| Text paragraph | Block | Complete source text or rendered Markdown |
| Code block | Block | Complete code source |
| Mermaid | Block | Complete diagram preview or source fallback |
| Reasoning | Block | Complete visible reasoning source |
| Unknown custom tool | Block | Adapter-provided custom Preview or complete summary fallback |

A Block without a specialized Preview falls back to its complete copy source. This fallback makes every valid Block useful in normal mode.

## Normal mode selection

Normal mode uses:

```rust
PreviewPolicy::FollowLatestBlock
```

The selected target is the last Block in `ReadingDocument`:

```text
ReadingDocument.blocks.last() -> PreviewTarget::Block
```

Rules:

1. A new Block becomes the target as soon as it enters the document.
2. Streaming updates to the newest Block refresh the same target rather than creating a new one.
3. Tool settlement refreshes the current Preview while preserving the Block ID.
4. History prepend inserts older Blocks before the existing document and does not change the target.
5. If the newest Block has no specialized Preview, the pane shows its complete copy source.
6. If the document has no Blocks, the pane shows an empty state.
7. The composer remains active.
8. Preview scrolling is local to the pane. A new target resets its scroll to the top.

## Reading View selection

Reading View uses:

```rust
PreviewPolicy::FollowReadingCursor
```

Target priority is:

```text
if ItemCursor exists
  PreviewTarget = current Item
else
  PreviewTarget = current Block
```

New transcript events continue to update `ReadingDocument`, but they do not move the Block cursor or replace the Preview target while Reading View is active.

When Reading View exits:

```text
policy = FollowLatestBlock
target = newest Block, if any
preview = resolve target
```

## Deferred Preview resolution

`e-tui` requests deferred data through a `UiAction`:

```rust
UiAction::ResolvePreview {
    request_id: PreviewRequestId,
    key: PreviewKey,
    revision: u64,
}
```

The executable or adapter responds through an event:

```rust
AgentEvent::Preview(PreviewEvent::Resolved {
    request_id: PreviewRequestId,
    key: PreviewKey,
    revision: u64,
    result: Result<PreviewContent, PreviewError>,
})
```

A late result is cached but may update the visible pane only when its key, revision, and request ID still match the current target.

```text
select Item A
  request A

select Item B
  request B

result A arrives
  cache A
  keep B visible

result B arrives
  cache B
  display B
```

Background tasks never mutate `PreviewPaneState` directly.

## Reading View entry

The selected Reading View binding enters Reading View.

```text
on selected Reading View binding
  refresh ReadingDocument
  refresh ReadingLayout at Reading View width
  collect eligible Blocks

  if no eligible Block exists
    remain in normal mode
    show a short notice
    return

  choose the Block nearest the viewport center
  set one Block cursor
  clear the Item cursor
  switch Preview policy to FollowReadingCursor
  resolve the Block Preview
  preserve the composer draft
```

### Initial Block selection

Let the viewport center be:

```text
viewport_center = viewport_top + viewport_height / 2
```

For each eligible Block that intersects the viewport, calculate the distance between the Block's visual center and the viewport center. Select the smallest distance. A tie selects the earlier Block.

If no eligible Block intersects the viewport, select the globally nearest Block.

## Cursor invariants

Normal mode has no Reading View cursors:

```text
BlockCursor = None
ItemCursor = None
```

Reading View has exactly one Block cursor and at most one Item cursor:

```text
BlockCursor = Some(exactly one BlockId)
ItemCursor = None or Some(exactly one ItemId)
```

When an Item cursor exists, it belongs to the current Block.

## Block mode

Block mode has a Block cursor and no Item cursor.

| Key | Behavior |
| --- | --- |
| `j` or Down | Move to the next eligible Block. |
| `k` or Up | Move to the previous eligible Block. |
| `l` or Right | Enter Item mode when the Block has Items. |
| `y` | Copy the current Block. |
| `Esc` | Exit Reading View. |

Entering Item mode selects the first Item in visual order unless a retained horizontal anchor identifies a nearer Item.

## Item mode

Item mode has both cursors. Navigation uses rendered Item geometry.

Directional candidate ranking is:

1. filter to candidates in the requested direction;
2. compare distance on the primary axis;
3. compare distance on the secondary axis;
4. use visual and document order as the final tie break.

### Up and down

`k` or Up selects the nearest Item above within the current Block. If none exists:

1. move the Block cursor to the previous Block;
2. select the Item nearest the previous horizontal position when that Block has Items;
3. otherwise clear the Item cursor and remain in Block mode.

`j` or Down applies the same rule toward the next Block.

### Left

`h` or Left selects the nearest Item to the left. If none exists, it leaves Item mode while keeping the Block cursor unchanged.

### Right

`l` or Right selects the nearest Item to the right. If no Item exists to the right on the current visual row, it selects the first Item on the nearest later row. If there is no later Item row, it does nothing.

### Escape

`Esc` leaves Item mode and keeps the current Block cursor. A second `Esc` from Block mode exits Reading View.

## Cursor-driven scrolling

Let visible transcript height be `H`:

```text
top_threshold = viewport_top + H / 3
bottom_threshold = viewport_top + 2 * H / 3
```

After cursor movement:

```text
if current Block enters the top third
  scroll up one page

if current Block enters the bottom third
  scroll down one page
```

Scrolling clamps to document bounds. Cursor identity is preserved by `BlockId`, not by a cached row number.

A resize or rewrap rebuilds `ReadingLayout`, locates the current Block again, and restores a visible anchor.

## Copy behavior

`y` copies the complete current Block in both Block mode and Item mode. It does not copy only the Item.

```text
on y
  find current ReadingBlock
  emit UiAction::WriteClipboard(block.copy.complete_source)
```

The copy source is independent of clipping, wrapping, folding, and Preview rendering. Code, table, and Mermaid source keeps its existing atomic semantics.

The first Reading View version does not support multi-Block selection. The old row range and anchor selection model is removed only after Reading View covers all required copy cases.

## Hover styling

The current Block uses:

- Night background;
- a Bark vertical rail in the left gutter.

The rail replaces a column from the existing outer margin. It does not insert a text glyph, change content width, or cause rewrapping.

```text
normal gutter: blank
hover gutter: Bark rail
content x and width: unchanged
```

Night is the base line background for the Block. Spans with an explicit local background, such as inline code chips, keep that explicit background.

The current Item uses a local highlight over its rendered fragments. At most one Block and one Item are highlighted.

## Focus and input routing

Reading View is a focus mode handled by the central input router. Events are not broadcast to all Regions.

Global precedence remains explicit:

```text
Help
  -> global transcript paging
  -> Input Page
  -> approval
  -> Reading View
  -> ordinary composer input
```

The exact position relative to approval and other blocking interactions must preserve current safety behavior. A blocking approval or Question Input Page should prevent entering Reading View unless the product behavior is changed deliberately.

The composer draft remains untouched while Reading View is open. Exiting returns to the same buffer, cursor, multiline state, and completion state.

## Preview scrolling

The first version does not give Preview a separate keyboard focus in Reading View because `h`, `j`, `k`, and `l` belong to semantic navigation. A target change resets Preview scroll to the top.

Independent Preview scrolling can be added later with distinct keys or a focus toggle. It must not overload the first version's navigation keys.

## Empty, loading, and error states

### No Blocks

Normal mode shows an empty Preview. The selected Reading View binding is rejected with a short notice.

### Deferred value loading

The Preview pane shows the selected target title and a loading indicator. The transcript cursor remains responsive.

### Resolution error

The Preview pane shows a bounded error message for the selected target. The Block remains navigable and copyable.

### Missing specialized Preview

The pane renders the Block's complete copy source. This is not an error.

## Key compatibility risk

Some terminals reserve `Ctrl+V` for paste before crossterm receives a key event. Implementation must verify the candidate in Windows Terminal, ConHost, and supported Linux terminals while bracketed paste still produces `Event::Paste` correctly.

If every supported terminal delivers `Ctrl+V`, it becomes the selected Reading View binding. Otherwise one documented alternate must pass the same gate and be used consistently in implementation, help text, and user documentation. This does not change the Reading View state model.
