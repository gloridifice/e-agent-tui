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

const HELP_MAX_WIDTH: u16 = 100;
const HELP_MIN_WIDTH: u16 = 48;
const HELP_MIN_HEIGHT: u16 = 12;

fn popup_extent(total: u16, minimum: u16, maximum: Option<u16>) -> u16 {
    if total <= 2 {
        return total;
    }
    let available = total - 2;
    let scaled = ((u32::from(total) * 4 + 4) / 5) as u16;
    let capped = maximum.map_or(scaled, |maximum| scaled.min(maximum));
    capped.max(minimum.min(available)).min(available)
}

pub(super) fn help_popup_rect(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let width = popup_extent(area.width, HELP_MIN_WIDTH, Some(HELP_MAX_WIDTH));
    let height = popup_extent(area.height, HELP_MIN_HEIGHT, None);
    ratatui::layout::Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(super) fn render_help_modal(
    frame: &mut Frame,
    config: &crate::Config,
    theme: &Theme,
    scroll: Option<&mut crate::interaction::HelpScrollState>,
) {
    use ratatui::{
        layout::{Alignment, Rect},
        widgets::Clear,
    };

    let popup = help_popup_rect(frame.area());
    if popup.width == 0 || popup.height == 0 {
        return;
    }
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(Line::styled(
            tr(config.language, "overlay.help.title"),
            theme.overlay.accent.style(),
        ))
        .title_alignment(Alignment::Center)
        .style(theme.overlay.background.style())
        .border_style(theme.overlay.border.style());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let content = Rect::new(
        inner.x.saturating_add(1),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    if content.width == 0 || content.height == 0 {
        return;
    }
    let footer_rows = usize::from(content.height > 1);
    let body_height = usize::from(content.height).saturating_sub(footer_rows);
    let body = Rect::new(content.x, content.y, content.width, body_height as u16);

    let mut next_unit = 0;
    let mut units = std::collections::HashMap::new();
    let options = crate::render::RenderOptions {
        language: config.language,
        collapse_rows: usize::MAX,
        mermaid_enabled: false,
        content_width: Some(usize::from(content.width)),
        ..Default::default()
    };
    let lines = crate::render::render_markdown(
        &crate::help::markdown(config),
        theme,
        &mut next_unit,
        &options,
        &mut units,
    )
    .into_iter()
    .map(|line| {
        if line.fill {
            line.line.patch_style(theme.markdown.code_block_bg.style())
        } else {
            line.line
        }
    })
    .flat_map(|line| crate::transcript_layout::wrap_line(line, usize::from(content.width)))
    .collect::<Vec<_>>();

    let offset = if let Some(scroll) = scroll {
        scroll.update_viewport(lines.len(), body_height);
        scroll.offset()
    } else {
        0
    };
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(offset)
                .take(body_height)
                .collect::<Vec<_>>(),
        )
        .style(theme.overlay.background.style()),
        body,
    );

    if footer_rows > 0 {
        let footer = Rect::new(content.x, content.y + body.height, content.width, 1);
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::help::footer(config),
                theme.surface.muted_text.style(),
            ))
            .alignment(Alignment::Center)
            .style(theme.overlay.background.style()),
            footer,
        );
    }
}
