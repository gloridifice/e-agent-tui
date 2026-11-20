use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    agent::{DelegatedStatus, DelegatedTask},
    theme::Theme,
};

pub(super) struct Strip<'a> {
    entries: Vec<&'a DelegatedTask>,
    hidden: usize,
    pub width: usize,
}

pub(super) fn fit(entries: &[DelegatedTask], budget: usize) -> Strip<'_> {
    let mut strip = Strip {
        entries: Vec::new(),
        hidden: entries.len(),
        width: 0,
    };
    if entries.is_empty() || budget < 4 {
        return strip;
    }
    // Outer padding, square, inner space, and one separator between entries.
    let width = |e: &DelegatedTask| 3 + e.label.width();
    let total = 1 + entries.iter().map(width).sum::<usize>();
    if total <= budget {
        strip.entries.extend(entries);
        strip.width = total;
        strip.hidden = 0;
    } else {
        strip.width = 3 + strip.hidden.ilog10() as usize + 1; // space, +N, space
        if strip.width > budget {
            strip.width = 0;
            return strip;
        }
        for running in [true, false] {
            for entry in entries
                .iter()
                .filter(|e| (e.status == DelegatedStatus::Running) == running)
            {
                if strip.width + width(entry) <= budget {
                    strip.entries.push(entry);
                    strip.hidden -= 1;
                    strip.width += width(entry);
                }
            }
        }
        strip
            .entries
            .sort_by_key(|e| entries.iter().position(|x| x.slot == e.slot));
    }
    strip
}

impl Strip<'_> {
    pub fn line(&self, theme: &Theme) -> Line<'_> {
        let palette = [
            theme.link,
            theme.running,
            theme.ok,
            theme.markdown.inline_code.fg,
            theme.markdown.strong.fg,
            theme.fg,
        ];
        let mut spans = vec![Span::raw(" ")];
        for e in &self.entries {
            spans.push(Span::styled(
                "■ ",
                Style::default().fg(palette[e.slot % palette.len()]),
            ));
            let style = match e.status {
                DelegatedStatus::Running => {
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
                }
                DelegatedStatus::Done => Style::default().fg(theme.dim),
                DelegatedStatus::Failed => Style::default()
                    .fg(theme.err)
                    .add_modifier(Modifier::UNDERLINED),
            };
            spans.extend([Span::styled(e.label.as_str(), style), Span::raw(" ")]);
        }
        if self.hidden > 0 {
            spans.push(Span::styled(
                format!("+{} ", self.hidden),
                Style::default().fg(theme.dim),
            ));
        }
        Line::from(spans)
    }
}
