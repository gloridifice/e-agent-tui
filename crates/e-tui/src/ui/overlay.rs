use super::*;
use crate::i18n::tr;

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

pub(super) fn help_overlay(config: &crate::Config, theme: &Theme) -> Vec<Line<'static>> {
    use crate::key_mapping::{Action::*, Scope::*};
    let style = Style::default().fg(theme.fg).bg(theme.bg_soft);
    let mut rows = vec![Line::styled(
        tr(config.language, "overlay.help.title"),
        style,
    )];
    let groups: &[(crate::key_mapping::Scope, &[crate::key_mapping::Action])] = &[
        (Global, &[PrintHelp, EnterReadMode]),
        (Global, &[ChooseModel, ChooseEffort, OpenSettings]),
        (Global, &[ResumeSession, TogglePreview]),
        (MessageIdle, &[Send]),
        (MessageWorking, &[SendAsap, SendAfterTurn]),
        (Message, &[NewLine, Paste]),
        (Message, &[CancelOrInterrupt]),
        (Message, &[ClearOrQuit]),
        (ReadMode, &[MoveUp, MoveDown, CopyBlock]),
        (ReadMode, &[MoveUpFast, MoveDownFast]),
        (ReadMode, &[EnterItems, Exit]),
        (ReadModeItem, &[BackToBlocks]),
        (Page, &[Confirm, Back]),
        (Approval, &[Allow, Deny]),
        (Help, &[Close]),
    ];
    for &(scope, actions) in groups {
        rows.push(Line::styled(
            crate::help::key_hints(config, scope, actions),
            style,
        ));
    }
    rows.push(Line::styled(tr(config.language, "key.help.more"), style));
    rows
}
