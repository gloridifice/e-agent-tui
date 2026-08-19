## MODIFIED Requirements

### Requirement: 增量缓存和复制语义不得退化
The display and Reading View migration MUST preserve streaming tail splice, activity range patches, final settle-color patches, width/generation layout cache, visible-window materialization, and original Markdown copy provenance.

#### Scenario: Assistant 流式 chunk
- **WHEN** a new chunk only extends the transcript's final assistant Block
- **THEN** the cache marks and splices only the tail without rebuilding the complete transcript

#### Scenario: 活动动画
- **WHEN** a spinner or settle transition changes an activity color
- **THEN** the cache patches only the matching `DisplayId` range and commits the exact final color before animation stops

#### Scenario: 原子内容复制
- **WHEN** Reading View copies a table, code, or Mermaid Block
- **THEN** the result comes from the stable unit's original Markdown source rather than rendered terminal characters

#### Scenario: Markdown 布局重新物化
- **WHEN** an assistant Block streams, terminal width changes, pane width changes, or expansion state changes
- **THEN** `transcript_layout` reuses the stable `DisplayId` unit-ID range and rematerializes atomic/raw-line provenance without storing Ratatui `RenderLine` values in the transcript store

## ADDED Requirements

### Requirement: Reading Document is an index over the single projection
The Reading Document SHALL derive semantic Blocks, Items, copy payloads, and Preview references from the canonical display projection and shared layout provenance. It MUST NOT retain a second message timeline or independently replay HostEvents.

#### Scenario: Projection replaces a surface
- **WHEN** canonical projection removes old display nodes and inserts a replacement
- **THEN** the Reading Document removes and inserts only the semantic units owned by those canonical nodes

#### Scenario: Cross-page activity correlates
- **WHEN** a tool lifecycle is completed from halves loaded across history pages
- **THEN** canonical projection produces one final activity and Reading View observes one corresponding stable Block

#### Scenario: Architecture guard scans transcript ownership
- **WHEN** source guards inspect production transcript storage
- **THEN** they find one public display path and no Reading-specific duplicate message store
