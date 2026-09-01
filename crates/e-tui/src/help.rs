//! Provider-neutral help content for local frontend presentation.

use crate::{
    agent::CommandDescriptor,
    command_catalog::{match_command_catalog, CommandCandidate, CommandSource},
};

const INTERACTION_HELP: &str = r#"# e 帮助

## 输入与控制

- `Enter`：发送
- `Shift+Enter`：换行
- `Ctrl+V`：粘贴图片或文本
- `Ctrl+Backspace` / `Ctrl+W`：删除前一个词
- `↑` / `↓`：行间移动，并在边界切换历史提示词
- `Esc`：中断对话或运行中的命令
- `Ctrl+C`：清空输入；空闲且输入为空时退出

## 视图与导航

- `Ctrl+Y`：进入阅读视图
- `Ctrl+P`：切换全屏预览
- `Ctrl+N`：打开续接会话 Input Page
- `Ctrl+H`：打开快捷帮助浮层
- `PageUp` / `PageDown` / 鼠标滚轮：滚动消息
- 输入 `/`：补全内置命令及当前运行时命令；使用 `Tab` 或 `↑` / `↓` 选择

## Input Page 与交互

- Input Page：方向键或 `hjkl` 移动焦点，`Enter` 执行，`Esc` 返回；编辑状态下 `hjkl` 输入文字
- 审批：`Y` 允许，`n` 拒绝
- 提问页：`h` / `l` 切换问题，`j` / `k` 切换选项，`Space` 选择，`Enter` 提交
- 阅读视图：`j` / `k` 块导航，`l` 进入项目，`h` / `j` / `k` / `l` 项目导航，`y` 复制完整块
"#;

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn append_commands(output: &mut String, heading: &str, commands: &[&CommandCandidate]) {
    if commands.is_empty() {
        return;
    }
    output.push_str("\n## ");
    output.push_str(heading);
    output.push_str("\n\n");
    for command in commands {
        let line = command.line.replace('`', "");
        let description = one_line(&command.description);
        output.push_str("- `");
        output.push_str(&line);
        output.push('`');
        if !description.is_empty() {
            output.push_str("：");
            output.push_str(&description);
        }
        output.push('\n');
    }
}

/// Build the complete local `/help` Markdown from authoritative catalogs.
pub(crate) fn markdown(integrated: &[CommandDescriptor]) -> String {
    let commands = match_command_catalog("", integrated);
    let builtins = commands
        .iter()
        .filter(|command| command.source == CommandSource::Builtin)
        .collect::<Vec<_>>();
    let integrated = commands
        .iter()
        .filter(|command| command.source == CommandSource::Integrated)
        .collect::<Vec<_>>();

    let mut output = INTERACTION_HELP.trim_end().to_owned();
    output.push('\n');
    append_commands(&mut output, "内置命令", &builtins);
    append_commands(&mut output, "当前运行时命令", &integrated);
    output
}
