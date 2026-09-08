//! Presentation-only highlighting for simple shell commands and command chains.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[derive(Clone, Copy)]
pub struct CommandColors {
    pub executable: Color,
    pub argument: Color,
    pub operator: Color,
}

impl CommandColors {
    fn style(self, role: Role) -> Style {
        let color = match role {
            Role::Executable => self.executable,
            Role::Argument | Role::Flag => self.argument,
            Role::Operator => self.operator,
        };
        let style = Style::default().fg(color);
        if role == Role::Flag {
            style.add_modifier(Modifier::ITALIC)
        } else {
            style
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Executable,
    Argument,
    Flag,
    Operator,
}

#[derive(Debug, PartialEq, Eq)]
struct Token<'a> {
    text: &'a str,
    role: Role,
}

/// Highlight complete source before the caller wraps or clips it. Source line
/// breaks remain separate rows; no prompt, padding, background, or border is added.
pub fn highlight(source: &str, colors: CommandColors) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default()];
    for token in tokens(source) {
        for part in token.text.split_inclusive('\n') {
            let text = part.strip_suffix('\n').unwrap_or(part);
            let text = if part.ends_with('\n') {
                text.strip_suffix('\r').unwrap_or(text)
            } else {
                text
            };
            if !text.is_empty() {
                lines
                    .last_mut()
                    .unwrap()
                    .spans
                    .push(Span::styled(text.to_owned(), colors.style(token.role)));
            }
            if part.ends_with('\n') {
                lines.push(Line::default());
            }
        }
    }
    lines
}

fn operator(source: &str) -> Option<(usize, bool)> {
    for redirect in ["&>>", "&>"] {
        if source.starts_with(redirect) {
            return Some((redirect.len(), true));
        }
    }
    let digits = source.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &source[digits..];
    for redirect in [">>", ">", "<"] {
        if let Some(tail) = rest.strip_prefix(redirect) {
            return Some((
                digits + redirect.len() + usize::from(tail.starts_with('&')),
                true,
            ));
        }
    }
    ["&&", "||", "|&", ";", "|", "&"]
        .into_iter()
        .find(|separator| source.starts_with(separator))
        .map(|separator| (separator.len(), false))
}

fn word_end(source: &str) -> (usize, Option<usize>) {
    let mut chars = source.char_indices();
    let mut quote = None;
    let mut equal = None;
    while let Some((offset, ch)) = chars.next() {
        if ch == '\\' && quote != Some('\'') {
            chars.next();
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() {
            match ch {
                '\'' | '"' => quote = Some(ch),
                ';' | '&' | '|' | '<' | '>' => return (offset, equal),
                ch if ch.is_whitespace() => return (offset, equal),
                '=' if equal.is_none() => equal = Some(offset),
                _ => {}
            }
        }
    }
    (source.len(), equal)
}

fn tokens(source: &str) -> Vec<Token<'_>> {
    let mut result = Vec::new();
    let mut rest = source;
    let mut executable = true;
    let mut flags = true;
    let mut redirect_target = false;
    while !rest.is_empty() {
        let whitespace = rest
            .find(|ch: char| !ch.is_whitespace())
            .unwrap_or(rest.len());
        let (length, role) = if whitespace > 0 {
            if rest[..whitespace].contains('\n') {
                executable = true;
                flags = true;
                redirect_target = false;
            }
            (whitespace, Role::Argument)
        } else if let Some((length, redirect)) = operator(rest) {
            redirect_target = redirect;
            if !redirect {
                executable = true;
                flags = true;
            }
            (length, Role::Operator)
        } else if rest.starts_with("\\\n") {
            (2, Role::Argument)
        } else if rest.starts_with("\\\r\n") {
            (3, Role::Argument)
        } else {
            let (length, equal) = word_end(rest);
            let word = &rest[..length];
            let role = if redirect_target {
                redirect_target = false;
                Role::Argument
            } else if executable {
                executable = false;
                Role::Executable
            } else if flags && word.starts_with('-') && word != "-" {
                if word == "--" {
                    flags = false;
                }
                if let Some(equal) = equal {
                    result.push(Token {
                        text: &word[..=equal],
                        role: Role::Flag,
                    });
                    result.push(Token {
                        text: &word[equal + 1..],
                        role: Role::Argument,
                    });
                    rest = &rest[length..];
                    continue;
                }
                Role::Flag
            } else {
                Role::Argument
            };
            (length, role)
        };
        result.push(Token {
            text: &rest[..length],
            role,
        });
        rest = &rest[length..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLORS: CommandColors = CommandColors {
        executable: Color::Red,
        argument: Color::Green,
        operator: Color::Blue,
    };

    fn with_role(source: &str, role: Role) -> Vec<&str> {
        tokens(source)
            .into_iter()
            .filter(|token| token.role == role)
            .map(|token| token.text)
            .collect()
    }

    #[test]
    fn flags_stop_at_values_and_reset_for_each_command() {
        let source = "cargo run -p e-pi --theme=ferra -- -literal&&rg -n x";
        assert_eq!(with_role(source, Role::Executable), ["cargo", "rg"]);
        assert_eq!(
            with_role(source, Role::Flag),
            ["-p", "--theme=", "--", "-n"]
        );
        let arguments = with_role(source, Role::Argument);
        for argument in ["run", "e-pi", "ferra", "-literal"] {
            assert!(arguments.contains(&argument));
        }
    }

    #[test]
    fn chains_and_redirects_distinguish_executables_from_targets() {
        let source = ">before cargo check&&echo ok>>out 2>&1;cat <out|head -n 5||echo no & wait|&sort &>>log";
        assert_eq!(
            with_role(source, Role::Executable),
            ["cargo", "echo", "cat", "head", "echo", "wait", "sort"]
        );
        assert_eq!(
            with_role(source, Role::Operator),
            [">", "&&", ">>", "2>&", ";", "<", "|", "||", "&", "|&", "&>>"]
        );
        let arguments = with_role(source, Role::Argument);
        for target in ["before", "out", "1", "log"] {
            assert!(arguments.contains(&target));
        }
    }

    #[test]
    fn quoted_and_escaped_operators_are_not_commands() {
        let source =
            r#""C:/Program Files/python.exe" --title="a && b" 'c;d|e>>f' a\&b "say \"hi\"""#;
        assert_eq!(
            with_role(source, Role::Executable),
            ["\"C:/Program Files/python.exe\""]
        );
        assert_eq!(with_role(source, Role::Flag), ["--title="]);
        assert!(with_role(source, Role::Operator).is_empty());
        assert!(with_role(source, Role::Argument).contains(&"\"a && b\""));
    }

    #[test]
    fn unicode_incomplete_quotes_and_escapes_preserve_every_byte() {
        for source in [
            "",
            "  \t",
            "echo 中文👩‍💻e\u{301}",
            "echo 'unfinished && cargo",
            "echo \"unfinished | rg",
            "echo trailing\\",
            "echo \\\n  --flag",
            "echo a\r\nrg b",
            "echo 'a\nb'",
            "echo --empty=",
        ] {
            assert_eq!(
                tokens(source)
                    .iter()
                    .map(|token| token.text)
                    .collect::<String>(),
                source
            );
        }
        assert_eq!(
            with_role("echo 'unfinished && cargo", Role::Executable),
            ["echo"]
        );
    }

    #[test]
    fn escaped_line_breaks_do_not_consume_executable_or_redirect_positions() {
        let source = "cargo check && \\\n echo ok > \\\r\n out; \\\n rg -- \\\n -literal";
        assert_eq!(with_role(source, Role::Executable), ["cargo", "echo", "rg"]);
        assert_eq!(with_role(source, Role::Flag), ["--"]);
        let arguments = with_role(source, Role::Argument);
        assert!(arguments.contains(&"out"));
        assert!(arguments.contains(&"-literal"));
    }

    #[test]
    fn multiline_commands_keep_lexical_state_and_display_rows() {
        let source = "cargo --release\r\necho 'one\ntwo' &&\nrg \\\n --glob=*.rs";
        assert_eq!(with_role(source, Role::Executable), ["cargo", "echo", "rg"]);
        let lines = highlight(source, COLORS);
        assert_eq!(
            lines.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [
                "cargo --release",
                "echo 'one",
                "two' &&",
                "rg \\",
                " --glob=*.rs"
            ]
        );
        assert_eq!(lines[4].spans[1].content, "--glob=");
        assert!(lines[4].spans[1]
            .style
            .add_modifier
            .contains(Modifier::ITALIC));
        assert!(!lines[4].spans[2]
            .style
            .add_modifier
            .contains(Modifier::ITALIC));
    }
}
