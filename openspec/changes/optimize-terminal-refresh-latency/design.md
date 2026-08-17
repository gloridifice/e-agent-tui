## Context

Rust 客户端当前用一个 50ms `tokio::time::interval` 唤醒主循环；Crossterm 键盘、鼠标和 resize 事件只在该唤醒或 WebSocket 收包之后通过 `poll(Duration::ZERO)` 排空。`dirty` 帧另受 30ms 条件限制，因此无网络流量时交互刷新实际上最多约 20 FPS，连续滚轮事件会每 50ms 成批跳动。入站消息也会无界排空，持续 chunk 流可能延后输入处理。

Ratatui 已对前后 buffer 做 cell diff，但 `CrosstermBackend<Stdout>` 仍按 cell 输出 ANSI；滚动导致可见 transcript 大部分 cell 改变，支持实时解析的终端可能展示尚未写完的半帧。当前隐藏硬件光标只避免 cursor travel，不提供帧原子性。目标平台 Windows Terminal 支持 DEC private mode 2026，Crossterm 0.28 已提供 synchronized update 命令。

`TranscriptRenderCache` 当前只有全局 `valid`、`tail_dirty` 和尾段长度。流式文本已能尾部拼接，但 `tick_spinners` 在任何活动存在时每 50ms 使整份缓存失效；配置的 `spinner_frame_ms` 尚未驱动时钟。折行在每帧对可见行重复计算，滚动 offset 却按未折行 cache line 计数，和“每格 3 个显示行”的交互契约不完全一致。

本变更仅涉及 Rust 客户端，不改变 WebSocket 协议、Node bridge、显示内容、copy source、历史分页或输入键位。实现必须继续遵守主循环 mutex guard 不跨 `.await` 的纪律，并保留“只克隆可见窗口”的性能边界。

## Goals / Non-Goals

**Goals:**

- 让键盘、鼠标和 resize 直接唤醒主循环，将交互事件到帧提交的正常上界从固定 50ms ticker 降到帧节流期限。
- 在支持 synchronized output 的终端上原子展示完整差量帧，并通过缓冲 writer 降低逐 cell 写入成本。
- 在持续网络流、动画和交互同时发生时保证公平性，保持消息顺序和文本完整性。
- 让动画按配置频率运行，只重算活动消息对应的缓存范围。
- 将折行后的 display row 作为滚动、viewport、历史锚点与 copy 布局共同使用的坐标，并缓存宽度相关布局。
- 提供低开销、显式启用的帧指标和可重复 benchmark，验证项目既有的 30ms 帧红线。

**Non-Goals:**

- 改变可见消息样式、快捷键、滚轮每格 3 行、自动跟随、Input Page 或 copy provenance 语义。
- 修改 bridge 协议或改变 chunk、snapshot、history 的 wire 格式。
- 在首批实现中引入 terminal scroll-region/hardware scrolling、GPU 渲染或替换 Ratatui/Crossterm。
- 为所有终端建立完整的能力协商数据库；不支持 DEC 2026 的终端继续走普通差量输出。

## Decisions

### 1. 用单一事件驱动调度器替代 50ms 输入轮询

为 Crossterm 启用 `event-stream`，让终端事件、bridge inbound、动画 deadline 和待提交帧 deadline 同时进入 `tokio::select!`。事件处理先在短生命周期状态锁内生成普通动作，释放锁后再发送异步消息，维持现有锁纪律。

调度器记录 dirty reason：`Interactive`、`Content`、`Animation`。交互事件在没有在途绘制时立即请求帧；若刚完成一帧，则合并到不晚于 16ms 的下一交互 deadline。流式内容最多约 30ms 合并一次；动画只在活动存在时按 `spinner_frame_ms` 安排。更高优先级的较早 deadline 覆盖较晚 deadline，绘制仍为单线程且不会重入。

选择 deadline 而不是把 ticker 简单改为 16ms，是为了在空闲时零唤醒，并让流式 chunk/动画保持可控频率。选择 EventStream 而不是永久 blocking 输入线程，是为了让关闭、错误和 resize 与 Tokio 生命周期统一。

### 2. 对入站处理设置公平预算

每次 bridge branch 先处理已收到的消息，再在“最多固定条数”和“最多约 2ms”两个预算中先到者处停止批量排空；未处理消息留在有界 channel，下一轮 select 继续。处理顺序不变，所有 chunk 仍逐一折叠，不丢弃文本、usage 或状态事件。

预算结束后调度器必须给已就绪的终端事件和帧 deadline 机会。选择有界批处理而不是只处理一条，是为了保留 burst coalescing；不在 bridge task 中预合并协议消息，以免跨事件类型破坏 surface/reducer 顺序。

### 3. 用统一终端 owner 提供缓冲、同步提交和可靠恢复

移除手工 setup 后再次调用 `ratatui::init()` 的双重初始化，建立一个拥有 raw mode、alternate screen、mouse/paste capture、cursor visibility、Ratatui terminal 和恢复状态的终端 owner。后端 writer 使用容量足以容纳常见帧的 `BufWriter<Stdout>`。

每次绘制执行以下事务：

1. best-effort 发送 `BeginSynchronizedUpdate`；
2. 执行 `terminal.draw`；
3. 移动仍保持隐藏的 IME anchor；
4. 无论前述步骤成功与否都尝试发送 `EndSynchronizedUpdate` 并 flush；
5. 返回首个真实错误。

不支持 DEC 2026 的终端会忽略控制序列，仍得到普通 Ratatui 差量帧。首批不发送同步能力查询，因为查询响应会进入同一输入流并增加启动状态机；若后续发现兼容问题，再增加显式 capability gate。panic/正常退出都只恢复一次终端。

### 4. 将帧指标放在可组合的 backend/writer 边界

新增默认关闭的运行期帧统计：backend wrapper 统计本帧变更 cell 数，counting writer 统计输出字节，主循环记录事件排队、state/update、render closure、backend draw/flush 和完整 frame transaction 时间。统计按固定窗口输出聚合的 count、P50、P95、P99 和 max，避免逐帧日志反过来干扰终端。

Tracy 构建增加 main-loop、inbound batch、cache rebuild/patch、layout、terminal write zones 与 frame marks；无头 benchmark 使用 TestBackend/计数 backend，不要求真实终端。普通构建且未启用诊断时只保留常数级时间戳或完全空转。

### 5. 保留扁平缓存，但增加消息范围和局部 patch

全量结构重建时，缓存除 `lines` 外记录每个 `Msg` 对应的可见 base-line range、gap ownership 和 generation。流式尾部继续使用现有 `tail_dirty`。动画时钟只把当前活动消息索引加入 `dirty_messages`；渲染器重新调用这些消息的公共 `styled_msg_lines`，若行数与旧 range 相同则原位替换，若发生意外结构变化则安全回退全量 invalidation。

该方案比立即改成树形/每消息独立缓存更小，并保持现有 surface replace、历史前插和统一 copy 布局。活动状态的结构变更仍是结构失效；纯颜色相位变化不是。状态栏呼吸灯不在 transcript cache 中，随动画帧直接重画。

动画 deadline 使用 `Config.spinner_frame_ms`，对过小或为零的旧配置做安全下限钳制。settle transition 在其结束前继续调度；到期时清除插值 source marker（保留完成时间哨兵）、提交一次精确目标色 patch 后停止动画唤醒。

### 6. 建立宽度相关的 display-row layout cache

base line 仍保存公共样式和 copy provenance；新增当前内容宽度与 base generation 对应的 display layout。布局为每个 base line 记录准确 wrapped-row count、前缀 offset，并仅物化当前 viewport 需要的 wrapped rows。折行计数和实际切分共享一个线性 grapheme 扫描器，跨 style span 保持 combining mark/emoji ZWJ 完整，避免当前对缩短后缀反复调用宽度计算。

滚动 offset 改为 display-row 坐标：滚轮精确移动 3 个可见行，PageUp/PageDown 移动当前 viewport 高度减一，follow 模式钉住最后的 display rows。copy layout 与 overlay 使用相同 display-row provenance，并由 width/generation cache 在按键与绘帧间复用；原始 Markdown/source 仍来自 `units`。历史前插在 layout 更新后按新增 display rows 调整 anchor。

终端内容宽度、主题中影响填充的配置或结构性 base generation 变化时，使相应 layout entry 失效；resize 不允许继续复用旧宽度缓存。tail splice 只替换受影响的 row-count suffix 并从旧 prefix 末端续算，不得使整份 layout 失效。全量 layout 只计算轻量 row count，屏幕每帧仍只克隆/物化可见窗口。

### 7. 以分阶段门槛决定是否采用硬件滚动

先交付事件调度、同步输出、BufWriter、入站公平性、动画局部 patch 和 display-row cache。若参考场景仍无法达到 P95 30ms，且指标显示主要成本来自滚动帧的变更 cell/ANSI 字节而不是 CPU，再单独提出 scroll-region 实现。

不在本变更首批直接操作 backend scrolling region，因为 Ratatui 的 previous/current buffer 必须与终端物理移动保持一致，且 transcript 高度会受 Input Page/accessory 改变；过早引入会显著增加错帧风险。

## Risks / Trade-offs

- **[Risk] EventStream 在某些 Windows/ConPTY 环境发生输入错误或关闭竞态** → 将输入错误转成可诊断事件，保持终端 owner 的幂等恢复，并用 Windows Terminal/传统控制台做冒烟。
- **[Risk] Begin 后绘制失败导致终端停留在同步模式** → frame transaction 在所有返回路径 best-effort 发送 End；终端恢复路径再次发送 End 后再离开 alternate screen。
- **[Risk] 不支持 DEC 2026 的终端显示转义副作用** → 使用 Crossterm 标准命令；默认按未知终端可忽略序列设计，并保留可禁用同步输出的环境回退开关。
- **[Risk] 16ms 交互 deadline 增加高频滚动 CPU** → 单帧不重入、连续事件合并、流式/动画使用更低频 deadline；指标验证而不是无条件全局 60 FPS。
- **[Risk] 局部 patch 的 range 因隐藏消息、gap 或状态变化失配** → 只允许行数稳定的纯展示 patch；任何 range/count 不一致立即全量重建，并用 activity adjacency/reasoning 隐藏回归覆盖。
- **[Risk] display-row 坐标迁移破坏历史锚点或 copy selection** → 所有 viewport/copy/provenance 统一消费同一 layout，增加折行 CJK、原子块、history prepend、overlay 的 UI 层测试。
- **[Trade-off] row-count layout 在 width/generation 变化时仍需扫描全部 base lines** → 扫描采用线性、无 Line clone 的计数路径；相比每帧重复折行，它把成本集中到结构或 resize 时。
- **[Risk] 性能测试受 CI 硬件波动影响** → 单测断言逻辑工作量（重建次数、可见行数、输出 cell 数）；30ms P95 作为 release benchmark/参考环境门槛，不用脆弱的普通 debug 单测判定。

## Migration Plan

1. 先加入指标、计数 backend 和基准场景，记录现有 80×40、160×50、240×70 与 1000+ 消息基线。
2. 引入单一终端 owner、BufWriter 和 synchronized frame transaction，保留可关闭同步输出的回退开关。
3. 切换 EventStream/deadline 调度和有界 inbound batch，以主循环回归测试验证公平性与锁释放。
4. 接入 `spinner_frame_ms` 和消息 range patch，保留全量重建 fallback。
5. 迁移 display-row layout、滚动、copy provenance 与历史 anchor，完成 UI/benchmark 验证。
6. 同步 README、AGENTS.md、设计/Tracy 文档后运行 `cargo test` 及 release 性能场景。

所有变更只影响本地客户端，没有数据或 wire migration。若出现兼容问题，可依次关闭 synchronized output、恢复普通 backend writer，或回退 deadline scheduler；缓存新字段为内存态，不影响配置和会话持久化。

## Open Questions

- 参考性能数据采集时是否需要把 Windows Terminal、ConHost 和 SSH 终端都列为发布门槛；首批至少以项目主目标 Windows Terminal 为硬门槛，其余作为兼容冒烟。
- synchronized output 的禁用开关最终采用环境变量还是高级只读配置；实现阶段优先使用环境变量，避免扩大 `/settings` 范围。
