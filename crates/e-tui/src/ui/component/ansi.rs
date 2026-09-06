//! Safe two-tone ANSI SGR mapping for command output preview.
//!
//! Parses already-bounded tool output through the `vte` terminal parser and
//! emits inert Ratatui spans: explicitly colored runs map to the `colored`
//! style, uncolored runs to the `plain` style, and SGR bold/italic modifiers
//! are preserved. Every other terminal effect (backgrounds, cursor movement,
//! OSC/hyperlinks/titles, erasure, and unsupported controls) is stripped, so no
//! escape byte or control sequence ever reaches the terminal backend.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use vte::{Params, Perform};

/// One line accumulator fed by the parser.
struct LineBuilder {
    spans: Vec<Span<'static>>,
    current: String,
    colored: Style,
    plain: Style,
    colored_flag: bool,
    bold: bool,
    italic: bool,
}

impl LineBuilder {
    fn new(colored: Style, plain: Style) -> Self {
        Self {
            spans: Vec::new(),
            current: String::new(),
            colored,
            plain,
            colored_flag: false,
            bold: false,
            italic: false,
        }
    }

    fn resolved_style(&self) -> Style {
        let mut style = if self.colored_flag {
            self.colored
        } else {
            self.plain
        };
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        style
    }

    fn flush_run(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.current);
        self.spans.push(Span::styled(text, self.resolved_style()));
    }

    fn finish_line(&mut self) -> Line<'static> {
        self.flush_run();
        Line::from(std::mem::take(&mut self.spans))
    }
}

struct Performer {
    line: LineBuilder,
    lines: Vec<Line<'static>>,
}

impl Perform for Performer {
    fn print(&mut self, c: char) {
        self.line.current.push(c);
    }

    fn execute(&mut self, byte: u8) {
        // Only a line feed ends a Preview row; every other C0 control is
        // dropped (tabs, carriage returns, backspace, bell, …).
        if byte == b'\n' {
            self.lines.push(self.line.finish_line());
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if action != 'm' || !intermediates.is_empty() || ignore {
            return;
        }
        // Finalize text printed before this SGR with its current style, then
        // apply the new state to text that follows.
        self.line.flush_run();
        // SGR parameters arrive as subparameter groups: `[38, 5, N]` and
        // `[38, 2, R, G, B]` are one group headed by 38, so treating any
        // group headed by 38 as "colored" naturally consumes its color spec.
        for group in params.iter() {
            let Some(&first) = group.first() else {
                continue;
            };
            match first {
                0 => {
                    self.line.colored_flag = false;
                    self.line.bold = false;
                    self.line.italic = false;
                }
                1 => self.line.bold = true,
                3 => self.line.italic = true,
                22 => self.line.bold = false,
                23 => self.line.italic = false,
                30..=37 | 90..=97 => self.line.colored_flag = true,
                38 => self.line.colored_flag = true,
                39 => self.line.colored_flag = false,
                48 => {}
                40..=47 | 49 | 100..=107 => {}
                _ => {}
            }
        }
    }
}

/// Parse one bounded output string into themed Preview lines. `colored` styles
/// runs under an explicit ANSI foreground; `plain` styles everything else.
pub fn terminal_lines(output: &str, colored: Style, plain: Style) -> Vec<Line<'static>> {
    // Normalize CR/LF so carriage returns never inject double rows.
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let mut performer = Performer {
        line: LineBuilder::new(colored, plain),
        lines: Vec::new(),
    };
    let mut parser = vte::Parser::new();
    parser.advance(&mut performer, normalized.as_bytes());
    // Flush a final unterminated line.
    performer.lines.push(performer.line.finish_line());
    performer.lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn style(fg: Color, bold: bool, italic: bool) -> Style {
        let mut s = Style::default().fg(fg);
        if bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        s
    }

    #[test]
    fn colored_run_maps_to_colored_and_plain_to_plain() {
        let colored = style(Color::Rgb(111, 93, 99), false, false); // bark
        let plain = style(Color::Rgb(77, 66, 75), false, false); // umber
        let lines = terminal_lines("plain \x1b[31mred\x1b[0m plain", colored, plain);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 3);
        assert_eq!(lines[0].spans[0].content, "plain ");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(77, 66, 75)));
        assert_eq!(lines[0].spans[1].content, "red");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Rgb(111, 93, 99)));
        assert_eq!(lines[0].spans[2].content, " plain");
    }

    #[test]
    fn bold_italic_survives_on_colored_run() {
        let colored = style(Color::Rgb(111, 93, 99), false, false);
        let plain = style(Color::Rgb(77, 66, 75), false, false);
        let lines = terminal_lines("\x1b[1;3;32mhi\x1b[0m", colored, plain);
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert_eq!(span.content, "hi");
        assert_eq!(span.style.fg, Some(Color::Rgb(111, 93, 99)));
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn crlf_collapses_and_controls_are_stripped() {
        let colored = style(Color::Rgb(111, 93, 99), false, false);
        let plain = style(Color::Rgb(77, 66, 75), false, false);
        let lines = terminal_lines("a\r\nb\rc\x1b]0;title\x07d", colored, plain);
        assert_eq!(
            lines
                .iter()
                .map(|l| l
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>())
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "b".to_string(), "cd".to_string()],
        );
    }
}
