use super::*;

pub(super) fn render_toast(frame: &mut Frame, message: &str, theme: &Theme) {
    let area = frame.area();
    if area.width < 4 || area.height < 3 {
        return;
    }
    let content = format!("✓ {message}");
    let width = (UnicodeWidthStr::width(content.as_str()) as u16)
        .saturating_add(4)
        .min(area.width);
    let right = area.x.saturating_add(area.width);
    let x = right.saturating_sub(width.saturating_add(1)).max(area.x);
    let y = if area.height > 3 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let popup = ratatui::layout::Rect::new(x, y, width, 3);
    frame.render_widget(ratatui::widgets::Clear, popup);
    frame.render_widget(
        Paragraph::new(Line::styled(content, theme.working_status.success.style()))
            .alignment(ratatui::layout::Alignment::Center)
            .block(
                Block::bordered()
                    .style(theme.overlay.background.style())
                    .border_style(theme.overlay.border.style()),
            ),
        popup,
    );
}

pub(super) fn help_overlay(theme: &Theme) -> Vec<Line<'static>> {
    let rows = [
        "帮助 — e",
        "Enter 发送   Shift+Enter 换行   Ctrl+V 粘贴   Ctrl+Backspace/Ctrl+W 删词   ↑↓ 行间移动/边界切换提示词",
        "Esc 中断对话/运行中命令   Ctrl+C 清空输入/空闲退出   /exit /q /quit 退出",
        "Ctrl+Y 阅读视图   Ctrl+P 预览   Ctrl+N 续接会话 Input Page   Ctrl+H 帮助",
        "输入 /：补全内置命令及当前会话自动接入的 DSH/插件命令   Tab/↑↓ 选择",
        "/settings 设置面板   /login 登录（API key/Proxy）   /new [模式] 新建会话   PgUp/PgDn/滚轮滚动消息",
        "鼠标: 拖动 Bark 分隔条调整消息栏比例；其他左键拖选 Transcript/Preview 可见文本，松开复制；阅读视图 y 复制完整原文",
        "/theme 切换主题   /model 选择模型   /effort 选择推理强度   /reload 重载配置/主题/技能   /skill:<名称> 注入技能",
        "Input Page: 方向键/hjkl 移动焦点（Settings ←→/hl 切页）   Enter 执行   Esc 返回；编辑时 hjkl 输入文字",
        "/resume 打开续接会话 Input Page（输入筛选、↑↓ 选择）/ /resume <会话ID> 直接切换",
        "审批: Y 允许 / n 拒绝   提问页: h/l/←→ 切换问题  j/k/↑↓ 切换选项  Space 选择/多选  Enter 下一题/提交",
        "阅读视图: j/k 块导航  l 进入项目  h/j/k/l 项目导航  y 复制完整块  Esc 返回/退出",
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
