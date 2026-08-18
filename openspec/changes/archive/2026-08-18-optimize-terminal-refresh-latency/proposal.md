> **状态：已完成。** 后续 `remediate-architecture-audit` 将 production transcript 从历史 `Msg`
> 存储迁到 `TranscriptStore`；本提案的事件驱动、同步输出、有界 inbound、tail/range patch 和
> display-row 性能契约继续有效，术语中的 `Msg` 仅描述当时实现。

## Why

当前客户端在滚轮滚动和持续输出时会出现明显的交互延迟与半帧刷新：输入只能随 50ms ticker 被轮询，滚动会让终端逐单元格展示尚未完成的差量帧，活动动画还会周期性使整份 transcript 缓存失效。需要建立可测量的刷新性能契约，并优先消除调度等待、终端 tearing 与长会话中的无效重建。

## What Changes

- 增加帧性能诊断，测量输入/网络事件到帧提交的延迟、渲染耗时、终端输出耗时、变更单元格/字节数和缓存重建范围。
- 将键盘、鼠标和 resize 接入事件驱动主循环，分离交互帧、流式帧与动画时钟，避免由固定 50ms ticker 引入输入等待。
- 对高频入站消息实施有界的数量或时间预算，在保持顺序与完整折叠的前提下避免输入和绘制饥饿。
- 将每个终端帧作为 synchronized output 原子提交，并使用缓冲写入；收敛重复的 raw mode/alternate-screen 初始化和可靠恢复路径。
- 让动画更新遵守 `spinner_frame_ms`，仅使活动消息对应的缓存段失效，不因颜色变化重建整个 transcript。
- 统一折行后的 display-row 布局与滚动坐标，缓存当前宽度下的可见布局，修复宽度变化失效，并避免长行重复扫描。
- 增加宽终端、长历史、持续流式输出、持续滚轮输入及不支持 synchronized output 的终端回退测试与基准。
- 仅在常规优化仍无法满足性能目标时，再评估 transcript scroll-region/hardware scrolling；该优化不作为首批实现的前置条件。

## Capabilities

### New Capabilities

- `terminal-render-performance`: 定义终端输入调度、帧节流与原子提交、入站公平性、增量缓存、折行滚动语义、性能观测和回退行为。

### Modified Capabilities

无。

## Impact

- 客户端主循环与终端生命周期：`client/src/main.rs`。
- transcript 缓存、动画失效和展示物化：`client/src/cache.rs`、`client/src/model.rs`、`client/src/presentation.rs`。
- 可见窗口布局、折行与滚动：`client/src/ui.rs`。
- 性能埋点：`client/src/profile.rs`、Tracy/无头 benchmark 与 UI 回归测试。
- 依赖配置可能为 Crossterm 启用异步事件流能力；WebSocket wire protocol 与 Node bridge 不变。
- 用户可见内容、键位、copy provenance、历史分页锚点和 Input Page 布局语义保持兼容。
