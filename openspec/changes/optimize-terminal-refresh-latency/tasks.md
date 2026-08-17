## 1. 建立性能基线与观测边界

- [x] 1.1 定义 80×40、160×50、240×70、1000+ 消息、持续流式、活动动画和连续滚动的 release benchmark fixtures，并记录当前基线。
- [x] 1.2 实现可组合的计数 backend/writer，分别统计 changed cells、输出字节、render/draw/flush/完整 frame transaction 时间，且不改变普通渲染语义。
- [x] 1.3 在 `profile.rs` 增加默认关闭的有界聚合统计（count、P50、P95、P99、max）和 Tracy main-loop/inbound/cache/layout/frame zones，避免逐帧日志干扰 TUI。
- [x] 1.4 为指标关闭态、聚合容量、计数准确性和 benchmark 输出字段增加单元测试。

## 2. 收敛终端生命周期与帧提交

- [x] 2.1 用单一 terminal owner 替代手工 setup 后再次 `ratatui::init()` 的重复初始化，统一 raw mode、alternate screen、mouse/paste capture、隐藏 cursor 与幂等恢复。
- [x] 2.2 将 Crossterm backend writer 改为有界 `BufWriter<Stdout>`，并接入第 1 组的字节计数边界。
- [x] 2.3 实现 synchronized frame transaction：Begin → Ratatui draw → 隐藏 IME anchor → End/flush，并确保所有错误和退出路径 best-effort 结束同步模式。
- [x] 2.4 增加禁用 synchronized output 的环境回退开关，以及支持、忽略、draw 失败、恢复重复调用等 writer/terminal 生命周期测试。

## 3. 改造事件驱动主循环

- [x] 3.1 为 Crossterm 启用异步事件流，把键盘、鼠标、paste 和 resize 作为 `tokio::select!` 的直接输入源。
- [x] 3.2 实现 `Interactive`、`Content`、`Animation` dirty reason 与单一 frame deadline 合并器，交互帧上限 16ms、流式帧约 30ms、空闲零周期唤醒。
- [x] 3.3 将现有键位、Input Page、picker、approval/question、copy mode 和普通输入分发迁入事件驱动分支，确保 mutex guard 不跨 `.await` 且行为不变。
- [x] 3.4 将 bridge inbound 排空改为条数与时间双重预算，保持消息顺序和完整 reducer 语义，并在预算后让输入及到期帧获得调度机会。
- [x] 3.5 使用可控时钟/事件源增加主循环测试，覆盖空闲滚轮即时唤醒、事件合帧、连续 inbound 公平性、resize、队列派发和 shutdown。

## 4. 动画与 transcript 局部缓存更新

- [x] 4.1 扩展 `TranscriptRenderCache`，记录 base generation、每个 `Msg` 的可见 line range/gap ownership 和待 patch 消息集合，同时保留 tail splice 与 history anchor 语义。
- [x] 4.2 在全量重建时生成消息 range，并实现行数稳定的原位 patch；range 或行数不匹配时自动回退结构性全量重建。
- [x] 4.3 修改动画时钟以识别当前活动消息，只标记对应 range，按带安全下限的 `Config.spinner_frame_ms` 调度，并在 settle 完成后停止动画 deadline。
- [x] 4.4 增加长 transcript 动画局部 patch、多个活动行、隐藏 reasoning 邻接、settle transition、tail chunk 和安全 fallback 的模型/UI 回归测试。

## 5. 统一 display-row 布局与滚动坐标

- [x] 5.1 将 `wrap_line` 与 `wrapped_rows` 的重复后缀扫描替换为共享的线性 Unicode 显示宽度布局器，并保持背景填充、CJK、跨 span 和原子块语义。
- [x] 5.2 在 transcript cache 中加入按 base generation 与内容宽度索引的 display-row count/prefix cache，并只物化或克隆 viewport 所需 wrapped rows。
- [x] 5.3 让 follow viewport 和 `render_transcript` 直接消费 display-row layout，继续保留尾部折行内容与输入栏前 gap。
- [x] 5.4 将滚轮、PageUp/PageDown 和 `ScrollState.offset` 迁移到 display-row 坐标，保证滚轮每格精确移动 3 个可见行。
- [x] 5.5 将 copy row provenance、cursor/selection overlay 与展开导航迁移到同一 display-row layout，复制内容仍来自原始 Markdown/units source。
- [x] 5.6 按新增 display rows 维护 history prepend anchor，并在 terminal width、主题填充或结构 generation 改变时正确失效布局。
- [x] 5.7 增加 ASCII/CJK 长行、用户卡、代码/表格/mermaid 原子块、宽度 resize、follow、分页锚点、copy overlay 和精确 3 行滚动的 TestBackend 回归测试。

## 6. 性能验证与硬件滚动决策

- [x] 6.1 增加逻辑工作量断言，证明普通滚动不重建 transcript、动画不重建 settled rows、流式只更新尾部且每帧只物化可见窗口。
- [x] 6.2 在 release 模式运行全部 benchmark，记录优化前后 scheduler、cache/layout、changed cells、ANSI bytes 与 P50/P95/P99，确认参考 Windows Terminal 环境 P95 完整帧不超过 30ms。
- [x] 6.3 在 Windows Terminal 上人工验证连续滚轮、持续输出、宽窗口、Input Page、copy mode 和 resize 不再出现半帧文字刷新或输入跳顿，并验证关闭同步输出的回退路径。
- [x] 6.4 根据指标记录 scroll-region/hardware scrolling 的 go/no-go：仅当 P95 仍超标且瓶颈明确为滚动输出量时另建变更，本变更不直接引入该高风险路径。

## 7. 文档与完整验证

- [x] 7.1 更新 `README.md` 的性能/兼容说明、`AGENTS.md` 的事件调度与缓存纪律、`docs/design.md` 的刷新架构，以及 `docs/tracy.md` 的新指标、benchmark 命令和实测结果。
- [x] 7.2 同步 `client/Cargo.toml` 的 Crossterm feature 与根目录 `Cargo.lock`，运行 `cargo fmt --all -- --check` 和适用的 Clippy 检查。
- [x] 7.3 运行 `cargo test` 全量测试；对任何已知偶发测试复跑确认，并保存 release benchmark 与 Windows Terminal 冒烟结论。
