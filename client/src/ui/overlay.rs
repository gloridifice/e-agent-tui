use super::*;

pub(super) fn help_overlay(theme: &Theme) -> Vec<Line<'static>> {
    let rows = [
        "帮助 — e",
        "Enter 发送   Shift+Enter 换行   ↑↓ 行间移动/边界切换提示词",
        "Esc 中断对话/运行中命令   Ctrl+C 清空输入/空闲退出   /exit /q /quit 退出",
        "Ctrl+B 复制模式   Ctrl+N 续接会话 Input Page   Ctrl+H 帮助",
        "输入 /：补全内置命令及当前会话自动接入的 DSH/插件命令   Tab/↑↓ 选择",
        "/settings 设置面板   /login 登录（API key/Proxy）   /new [模式] 新建会话   PgUp/PgDn/滚轮滚动消息",
        "/theme 切换主题   /model 选择模型   /reload 重载配置/主题/技能   /skill:<名称> 注入技能",
        "Input Page: 方向键/hjkl 移动焦点   Enter 执行   Esc 返回；编辑时 hjkl 输入文字",
        "/resume 打开续接会话 Input Page（输入筛选、↑↓ 选择）/ /resume <会话ID> 直接切换",
        "审批: Y 允许 / n 拒绝   提问: ←→ 切换选项  Enter 选中/下一项(最后一项确定)  Esc 取消",
        "复制模式: hjkl 移动  V 行选  Ctrl+V 块选  y 复制  Esc 退出",
        "q/Esc 关闭帮助",
    ];
    rows.iter()
        .map(|r| {
            Line::from(Span::styled(
                (*r).to_string(),
                Style::default().fg(theme.fg).bg(theme.bg_soft),
            ))
        })
        .collect()
}
