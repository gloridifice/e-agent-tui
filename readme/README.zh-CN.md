<div align="center">

<img src="logo.png" alt="e 标志" width="128">

# e

</div>

<div align="center">

[English](../README.md) | 简体中文 | [日本語](README.ja-JP.md) | [Français](README.fr-FR.md)

</div>

`e` 是面向编程智能体的终端界面：`dshe` 连接 [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness)，`pie` 通过官方 RPC 接口运行 Pi，不替换任何一方的智能体运行时。

> 项目仍处于早期开发阶段，主要支持 Windows 和 PowerShell 环境；同时提供 Linux x64 预编译程序。

https://github.com/user-attachments/assets/45a75c55-7e3b-4af3-b1e2-1c6306b80358

## 快速开始

### 安装

根据使用的智能体选择前端。以下命令需要 [Node.js 和 npm](https://nodejs.org/) 以及 [Rust 和 Cargo](https://rustup.rs/)。DSH 还需要 [Git](https://git-scm.com/) 和 pnpm。

**DeepSeek Harness — `dshe`**

```bash
npm install --global pnpm @deepseek-ai/dsh
cargo install e-dsh --locked
dshe setup
```

**Pi — `pie`**

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi --locked
```

不想安装 Rust？从 [GitHub Releases](https://github.com/gloridifice/e/releases) 下载 Windows x64 ZIP 或 Linux x64 tarball，用 `SHA256SUMS` 校验后，将可执行文件解压到 `PATH` 中的目录。每个压缩包都包含两个前端。仍需按上文安装所选智能体的 npm 依赖；使用 DSH 还需要 Git，并执行 `dshe setup`。

在项目目录中运行 `dshe` 或 `pie`，然后通过 `/login` 配置模型服务商的访问凭据。`dshe` 会自动启动专用 DSH 服务；`pie` 复用 Pi 原生的凭据、模型、扩展和会话文件。

在任一前端中使用 `/help` 查看键盘操作，使用 `pie --help` 查看 Pi 启动选项。兼容性详情见 [DSH 配置与升级](../doco/guides/dsh-integration.md)和 [Pi 运行时要求](../doco/guides/development.md#prerequisites)。

### 更新

重新执行所选前端对应的 npm 和 Cargo 安装命令。若使用预编译程序，下载最新版本并替换可执行文件。

更新 DSH 或 `dshe` 后，需再次运行 `dshe setup`，并重启正在运行的 DSH 服务以加载新版桥接插件。`/reload` 不能代替这次重启。

## 功能

- 流式 Markdown 渲染，支持代码语法高亮、表格、Mermaid 图表和 diff 预览。
- 可调整宽度的 Preview 面板和 Reading 模式，便于查看消息与工具输出。
- 多行编辑、输入历史、命令与路径补全，以及提示词队列。
- 模型与推理强度选择、可复用的提示词宏，以及可配置的上下文压缩模型。
- Pi 原生认证、MCP 管理，以及会话恢复、分叉和克隆。
- 本地 shell 命令不会进入模型上下文或智能体历史；命令以非交互方式运行，使用当前用户的权限。
- 执行历史展示操作耗时和 token／费用时间线。导出的元数据可能包含敏感的命令参数。
- 自定义主题、可配置的快捷键，以及英文或简体中文界面。

## 配置

通过 `/settings` 调整常用设置，或用 `/econfig` 查找两个前端共用的配置目录：

- Windows：`%APPDATA%\e`
- Linux：`$XDG_CONFIG_HOME/e`（默认 `~/.config/e`）
- macOS：`~/.config/e`

设置保存在 `config.toml` 中；设置 `language = "zh-CN"` 可启用简体中文。默认语言和回退语言均为英文。智能体凭据和会话文件仍由 DSH 或 Pi 管理。

参照[快捷键指南](../doco/guides/key-mapping.md)和[默认绑定](../crates/e-tui/assets/default_key_mapping.toml)自定义 `key_mapping.toml`。自定义主题的 TOML 文件放在 `themes/` 下，可参考[内置主题](../crates/e-tui/assets/themes/)。`/reload` 会重新加载配置、主题和后端资源。

## 从源码构建

需要 Git 和 Rust/Cargo，运行时仍需满足上文的依赖要求。

```bash
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release --locked -p e-dsh -p e-pi --bins
```

可执行文件位于 `target/release/`。从本地仓库安装、修改桥接插件和构建性能分析版本的方法见[开发指南](../doco/guides/development.md)。

## 许可证

可选择按 [MIT](../LICENSE-MIT) 或 [Apache-2.0](../LICENSE-APACHE) 许可证使用。

## 开发

欢迎提交 Issue。项目仍处于原型阶段，暂不接受 Pull Request。

- [文档索引](../doco/README.md)
- [开发指南](../doco/guides/development.md)
- [架构](../doco/architecture.md)
- [测试与验证](../doco/guides/testing.md)
- [问题排查](../doco/guides/troubleshooting.md)
