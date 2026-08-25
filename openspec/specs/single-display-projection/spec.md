# single-display-projection Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: Transcript 只存储公共显示表面
应用 transcript 状态 MUST 只存储 `ActivityRow`、`TranscriptBlock`、`ContentCard` 或由这些表面组成的显式 composite；旧 `Msg` 工具、Thinking、文件组、System、Error 等兼容变体 MUST 被删除。

#### Scenario: 核心事件完成投影
- **WHEN** user、assistant、reasoning、tool、file、retry、command、workflow、compaction 或 lifecycle HostEvent 被应用
- **THEN** transcript 中新增或更新的节点只使用公共显示表面，UI 不再执行旧消息到表面的临时转换

#### Scenario: 非 transcript 状态
- **WHEN** title、usage、preset、todo、goal、plan、approval 或 question 事件到达
- **THEN** 它更新对应页面、accessory 或 session state，而不会创建未声明的 transcript 消息类型

### Requirement: EventProjector 是唯一事件显示边界
HostEvent 到 transcript mutation 的分类和关联 MUST 只通过 typed projector/reducer 完成；原始 `serde_json::Value` MUST 不进入应用 reducer，render 模块 MUST 不接收 HostEvent。

#### Scenario: 已知事件家族
- **WHEN** 一个已知 HostEventKind 被处理
- **THEN** 对应事件家族 reducer 产生 typed display、surface、page、accessory 或 ignore effect，并由状态层应用

#### Scenario: 未知 surface append
- **WHEN** 未知事件带合法 append surface metadata
- **THEN** projector 产生有界 `TranscriptBlock` fallback，且不会把任意原始 payload 保存到 transcript

#### Scenario: 未知或损坏 replace
- **WHEN** 未知事件带 replace surface operation 或 surface metadata 无效
- **THEN** 系统产生非破坏性兼容错误，并且不会静默构造已知错误的 transcript

### Requirement: Surface 与历史语义保持一致
单轨显示状态 MUST 保留 append/replace ownership、shadowed seq、跨页 lifecycle half、稳定插入位置和有效显示行锚点语义。

#### Scenario: 后加载的 shadowed 历史
- **WHEN** 初始快照包含 replacement，随后历史页返回其已遮蔽的旧 surface nodes
- **THEN** 旧节点不会复活，replacement 保持在原 surface 位置

#### Scenario: 跨页活动配对
- **WHEN** tool、command、retry、Code Mode 或 workflow 的结束半先于开始半加载
- **THEN** projector 暂存关联结果，并在开始半到达后直接构造最终且不重复的活动状态

#### Scenario: 历史前插保持视口
- **WHEN** 一页历史在 transcript 顶部增加公共显示节点
- **THEN** viewport 仅按实际新增 display rows 平移，用户当前看到的内容保持稳定

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

### Requirement: 兼容层删除具有可验证完成条件
Display 迁移只有在生产代码不再声明旧 transcript `Msg` enum、不再包含 compatibility reducer、且 UI 不再匹配事件专用旧变体时 SHALL 被视为完成。

#### Scenario: 完成迁移检查
- **WHEN** 执行架构与源码守卫测试
- **THEN** 旧 `Msg` transcript 声明、`reduce_host_event` 兼容路径和旧变体 UI 分支均不存在，OpenSpec 清理任务才可保持完成状态

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

