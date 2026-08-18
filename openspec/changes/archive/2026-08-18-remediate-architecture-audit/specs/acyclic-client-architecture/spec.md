## ADDED Requirements

### Requirement: 生产模块依赖图无环
Rust 客户端的生产模块依赖图 MUST 不包含强连通分量；测试模块和组合根的高 fan-out 不得被误判为循环依赖。

#### Scenario: 架构检查通过
- **WHEN** 对 `client/src` 的生产 `use crate::...` 依赖执行架构检查
- **THEN** 检查得到零个多节点强连通分量，并以成功状态退出

#### Scenario: 禁止反向依赖被发现
- **WHEN** 叶子模块新增一条指向其上层控制器的依赖并形成环
- **THEN** 架构检查 MUST 失败并报告组成该环的模块路径

### Requirement: 命令目录与执行解耦
内置命令声明、目录合并、排序和参数补全 MUST 由不依赖 `InputState`、`AppState`、copy mode、Input Page 或异步 sender 的命令目录模块拥有；输入与命令执行模块 SHALL 只单向依赖该目录。

#### Scenario: 输入刷新命令建议
- **WHEN** 输入缓冲区包含内置命令或 bridge 下发的集成命令前缀
- **THEN** 输入模块通过命令目录得到与迁移前相同的候选、说明、来源和补全形式，而命令目录不读取或修改输入状态

#### Scenario: 执行本地命令
- **WHEN** runtime command dispatcher 执行一个已注册的内置命令
- **THEN** 它使用目录返回的动作标识产生运行时效果，且命令目录不依赖 dispatcher

### Requirement: 页面公共原语位于叶子层
焦点图、方向键归一化、文本编辑、viewport 和通用页面 outcome/effect MUST 位于 `page_core`；login、settings、model、theme、resume 页面 SHALL 依赖 `page_core`，而 `page_core` MUST 不导入任何具体页面。

#### Scenario: 页面焦点导航
- **WHEN** 任一具体 Input Page 使用方向键或 `hjkl` 移动焦点
- **THEN** 导航由 `page_core` 完成，并保持现有禁用项跳过和稳定 id reconcile 行为

#### Scenario: 文本编辑保留 hjkl
- **WHEN** login、settings 或 resume 页面处于文本编辑状态
- **THEN** `hjkl` 作为普通字符进入编辑器，具体页面无需反向调用 Input Page controller

### Requirement: 布局与复制共享无 UI 反向依赖
显示行布局和 copy provenance MUST 由独立 transcript layout 模块拥有；`ui` 与 `copy` SHALL 消费同一布局结果，`copy` MUST 不调用 `ui`。

#### Scenario: 复制当前显示行
- **WHEN** 用户在 copy mode 选择折行后的 Markdown、表格、代码或 Mermaid 行
- **THEN** 复制模块从共享布局获得与屏幕一致的全局行和原始来源，且依赖图中不存在 `copy → ui`

#### Scenario: 宽度变化
- **WHEN** 终端宽度改变并使 transcript 重新折行
- **THEN** UI 与 copy mode 在同一 width/generation 上失效并重新使用共享布局

### Requirement: 组合根只负责编排
`main.rs` MAY 依赖多个具体适配器，但 SHALL 只负责生命周期、事件选择、deadline 和效果执行；页面、命令、协议事件或复制行为的业务分支 MUST 位于可独立测试的处理模块。

#### Scenario: 新增页面消息处理
- **WHEN** 新增一个已有协议内的 Input Page 状态刷新分支
- **THEN** 变更落在页面或 bridge-message handler 中，而不需要向 `main.rs` 的事件循环增加页面专用业务逻辑
