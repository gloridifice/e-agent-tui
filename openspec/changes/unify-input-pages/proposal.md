## Why

配置相关页面目前分为两套交互形态：`/settings`、`/login` 替代输入栏，而 `/model`、`/theme` 使用浮窗；它们还分别维护导航、焦点、编辑和渲染逻辑，导致视觉与键位不一致并增加维护成本。需要引入统一的 Input Page 能力，使配置页面共享页面容器、单焦点导航和执行语义。

## What Changes

- 新增统一的 **Input Page** 页面接口和生命周期，同一时刻只允许一个 Input Page 打开。
- 所有 Input Page 替代用户输入框，在页面底部区域渲染，不再使用浮窗。
- Input Page 统一使用上下各 1 行、左右各 2 列的内边距，并共享背景、标题、正文、页脚、加载和错误布局。
- 引入单一稳定焦点：方向键及 `hjkl` 在可执行元素间移动，`Enter` 执行当前元素，`Esc` 取消编辑、返回上级或关闭页面。
- 区分浏览与文本编辑模式；文本编辑期间 `hjkl` 作为普通字符输入，不触发焦点导航。
- 将 `/settings`、`/login`、`/model`、`/theme` 迁移到 Input Page；会话选择器、帮助、复制模式、审批和问题栏不在本次范围内。
- `/model` 改为单焦点双栏页面，区分当前已应用模型与当前焦点，并在异步目录刷新时尽量保持稳定焦点。
- `/login` 的可聚焦元素必须具有有效的 Enter 行为；已有代理项提供删除操作，不再出现“可选中但无动作”的行。
- 收敛主循环中分散的页面状态、按键分支、渲染入口和服务端消息路由。
- 增加焦点导航、统一内边距、页面替换输入栏、异步刷新及小终端布局的回归测试，并同步更新用户与设计文档。

## Capabilities

### New Capabilities
- `input-page`: 定义配置页面的统一容器、布局、焦点导航、编辑模式、动作分发，以及 settings/login/model/theme 的可见交互行为。

### Modified Capabilities

无。

## Impact

- 主要影响 Rust 客户端：`client/src/main.rs`、`client/src/ui.rs`、`client/src/settings.rs`、`client/src/login.rs`、`client/src/runtime_command.rs`、`client/src/lib.rs`，并新增 Input Page、模型页和主题页相关模块。
- `/model`、`/theme` 从浮窗改为输入区替代页面；`/settings` 页签及 `/login` 代理项的焦点/执行行为会发生可见变化。
- 沿用现有 `ClientMessage`/`ServerMessage`，预计不修改 WebSocket 协议或 Node bridge。
- 需要同步更新 `README.md`、`AGENTS.md`、`docs/design.md` 和 TUI 帮助文本。
- 不引入新的运行时依赖。
