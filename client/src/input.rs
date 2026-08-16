//! Input-line state and key handling (design §4.1, D23/D24).
//!
//! The input bar is a borderless Ash block: one margin row above, the text
//! area (1 row, or up to 3 scrolling rows in multiline mode), one margin row
//! below. Pastes over the placeholder threshold render as `[N text pasted]`.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;

/// Built-in slash commands (name, one-line description) for the suggestion
/// popup.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "显示帮助"),
    ("/settings", "打开设置面板"),
    ("/login", "登录设置（API key / 账号 / proxy）"),
    ("/new", "新建会话（空格后可选模式）"),
    ("/resume", "切换会话"),
    ("/compact", "压缩上下文"),
    ("/goal", "目标管理"),
    ("/plan", "计划模式"),
    ("/copy", "进入复制模式"),
    ("/clear", "清空会话列表"),
    ("/exit", "退出客户端"),
    ("/q", "退出客户端"),
    ("/quit", "退出客户端"),
];

/// One selectable `/new` mode: an agent preset pushed by the bridge's
/// `presets` roster message (broken presets are filtered out before they
/// land here).
#[derive(Debug, Clone)]
pub struct NewMode {
    pub id: String,
    /// Display name; the popup falls back to the id when absent.
    pub name: Option<String>,
    pub description: Option<String>,
}

pub struct InputState {
    pub buf: String,
    /// Cursor as a char index into `buf`.
    pub cursor: usize,
    pub history: Vec<String>,
    pub hist_idx: Option<usize>,
    pub multiline: bool,
    /// Draft saved when entering multiline mode from a single line.
    pub draft: Option<String>,
    /// Ctrl+R history search, when active.
    pub search: Option<SearchState>,
    /// Paste placeholder threshold (D24, config-driven).
    pub paste_placeholder_chars: usize,
    /// The buffer is one atomic paste block (a paste over the threshold):
    /// it renders as `[N text pasted]` and the cursor can never enter it.
    pub pasted: bool,
    /// Enter sends immediately (D30); false = Enter inserts a newline and
    /// Ctrl+Enter sends.
    pub enter_sends: bool,
    /// History cap (D28).
    pub history_limit: usize,
    /// `/new <mode>` candidates from the bridge's `presets` roster message;
    /// empty until the roster arrives.
    pub new_modes: Vec<NewMode>,
    /// Slash-command suggestion popup, when open.
    pub suggest: Option<Suggestion>,
}

pub struct SearchState {
    pub query: String,
    pub sel: usize,
}

/// Suggestion popup state: the ranked rows for the typed query, the
/// highlighted selection, and the raw query (kept for Esc restore while
/// the user navigates and the buffer shows the filled command).
pub struct Suggestion {
    pub query: String,
    pub sel: usize,
    /// Ranked fill-in rows (what navigation fills and Enter sends):
    /// command lines like `/settings`, or `/new <mode-id>` lines.
    pub matches: Vec<String>,
    /// Per-row description column, parallel to `matches` (the command
    /// description or the mode's display name).
    pub descriptions: Vec<String>,
    /// True = mode rows (header "模式"); false = command rows (header "命令").
    pub modes: bool,
}

impl InputState {
    pub fn new(config: &Config) -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_idx: None,
            multiline: false,
            draft: None,
            search: None,
            paste_placeholder_chars: config.paste_placeholder_chars,
            pasted: false,
            enter_sends: config.enter_sends,
            history_limit: config.history_limit,
            new_modes: Vec::new(),
            suggest: None,
        }
    }

    /// Insert pasted text at the cursor. Content over the threshold becomes
    /// one atomic paste block: it renders as `[N text pasted]`, the cursor
    /// skips across it as a whole, and Backspace removes the whole block.
    pub fn paste(&mut self, text: &str) {
        for c in text.chars() {
            self.buf.insert(char_to_byte(&self.buf, self.cursor), c);
            self.cursor += 1;
        }
        if self.buf.chars().count() > self.paste_placeholder_chars {
            self.pasted = true;
            // The cursor may never sit inside the block.
            self.cursor = self.buf.chars().count();
        }
    }
}

/// What the UI should do after a key press.
#[derive(Debug, PartialEq)]
pub enum InputAction {
    None,
    /// Send the committed buffer as an ordinary message.
    Send(String),
    /// Send the committed buffer as a slash command line.
    Command(String),
    /// Interrupt the agent (Ctrl+C while running).
    Interrupt,
    /// Quit the TUI (Ctrl+C while idle).
    Quit,
    /// Enter copy mode (D12).
    CopyMode,
    /// Toggle multiline mode.
    ToggleMultiline,
}

/// Byte index of the char at `idx` in `s` (or `s.len()` when `idx` is past
/// the end). `InputState::cursor` is a char index; `String::insert`/`remove`
/// and slicing need a byte index — mixing the two panics on multi-byte (CJK)
/// input.
fn char_to_byte(s: &str, idx: usize) -> usize {
    s.char_indices().nth(idx).map(|(i, _)| i).unwrap_or(s.len())
}

/// Ranked fuzzy match against the command names: prefix matches first, then
/// substring matches, then subsequence matches; each group alphabetical. An
/// empty query returns every command.
pub fn match_commands(query: &str) -> Vec<&'static str> {
    let q = query.to_lowercase();
    let mut prefix: Vec<&'static str> = Vec::new();
    let mut substring: Vec<&'static str> = Vec::new();
    let mut fuzzy: Vec<&'static str> = Vec::new();
    for (cmd, _) in COMMANDS {
        let name = cmd.trim_start_matches('/').to_lowercase();
        if q.is_empty() {
            prefix.push(cmd);
        } else if name.starts_with(&q) {
            prefix.push(cmd);
        } else if name.contains(&q) {
            substring.push(cmd);
        } else if is_subsequence(&q, &name) {
            fuzzy.push(cmd);
        }
    }
    prefix.sort_unstable();
    substring.sort_unstable();
    fuzzy.sort_unstable();
    prefix.extend(substring);
    prefix.extend(fuzzy);
    prefix
}

/// Every char of `q` appears in `name` in order (classic fuzzy subsequence).
fn is_subsequence(q: &str, name: &str) -> bool {
    let mut chars = name.chars();
    q.chars().all(|c| chars.any(|n| n == c))
}

/// Ranked fuzzy match against the `/new` modes: id-prefix matches first,
/// then id/display-name substring matches, then subsequence matches; each
/// group keeps the roster order. An empty query returns every mode.
pub fn match_new_modes<'a>(query: &str, modes: &'a [NewMode]) -> Vec<&'a NewMode> {
    let q = query.to_lowercase();
    let mut prefix: Vec<&NewMode> = Vec::new();
    let mut substring: Vec<&NewMode> = Vec::new();
    let mut fuzzy: Vec<&NewMode> = Vec::new();
    for mode in modes {
        let id = mode.id.to_lowercase();
        let name = mode.name.as_deref().unwrap_or("").to_lowercase();
        if q.is_empty() {
            prefix.push(mode);
        } else if id.starts_with(&q) {
            prefix.push(mode);
        } else if id.contains(&q) || (!name.is_empty() && name.contains(&q)) {
            substring.push(mode);
        } else if is_subsequence(&q, &id) || (!name.is_empty() && is_subsequence(&q, &name)) {
            fuzzy.push(mode);
        }
    }
    // Roster order is the declared `order` — keep it stable inside each
    // group instead of re-sorting alphabetically.
    prefix.extend(substring);
    prefix.extend(fuzzy);
    prefix
}

impl InputState {
    pub fn handle_key(&mut self, key: &KeyEvent, idle: bool) -> InputAction {
        // Ctrl+C: clear the input bar; only an empty bar while idle quits.
        // (Esc is the interrupt key now.)
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if !self.buf.is_empty() {
                self.clear();
                self.search = None;
                self.multiline = false;
                self.draft = None;
                return InputAction::None;
            }
            if idle {
                return InputAction::Quit;
            }
            return InputAction::None;
        }
        // Ctrl+B enters copy mode, but only with an empty buffer (D12).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('b') {
            if self.buf.is_empty() && self.search.is_none() {
                return InputAction::CopyMode;
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            return InputAction::None;
        }
        // Ctrl+R: reverse history search (design §4.1).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            if !self.multiline {
                self.search = Some(SearchState { query: String::new(), sel: 0 });
            }
            return InputAction::None;
        }

        // History search mode owns the keys.
        if self.search.is_some() {
            let mut search = self.search.take().unwrap();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    return InputAction::None;
                }
                KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    let matches = self.matching_history(&search.query);
                    if let Some(entry) = matches.get(search.sel) {
                        self.buf = entry.clone();
                        self.cursor = self.buf.chars().count();
                    }
                    return InputAction::None;
                }
                KeyCode::Up => {
                    let n = self.matching_history(&search.query).len();
                    if n > 0 {
                        search.sel = (search.sel + n - 1) % n;
                    }
                }
                KeyCode::Down => {
                    let n = self.matching_history(&search.query).len();
                    if n > 0 {
                        search.sel = (search.sel + 1) % n;
                    }
                }
                KeyCode::Char(c) => {
                    if !c.is_ascii_control() {
                        search.query.push(c);
                        search.sel = 0;
                    }
                }
                KeyCode::Backspace => {
                    search.query.pop();
                    search.sel = 0;
                }
                _ => {}
            }
            self.search = Some(search);
            return InputAction::None;
        }

        // Slash-command suggestion popup: navigation owns the keys while open.
        if self.suggest.is_some() {
            match key.code {
                KeyCode::Esc => {
                    // Restore what was typed before the fill.
                    let query = self.suggest.take().map(|s| s.query).unwrap_or_default();
                    self.buf = query.clone();
                    self.cursor = query.chars().count();
                    return InputAction::None;
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                    let cmd = {
                        let s = self.suggest.as_mut().unwrap();
                        let n = s.matches.len();
                        if key.code == KeyCode::Up {
                            s.sel = (s.sel + n - 1) % n;
                        } else {
                            s.sel = (s.sel + 1) % n;
                        }
                        s.matches[s.sel].to_string()
                    };
                    // Auto-fill the selected command into the input bar.
                    self.buf = cmd.clone();
                    self.cursor = cmd.chars().count();
                    return InputAction::None;
                }
                KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                    // Send the highlighted command (Enter accepts it even
                    // under the Ctrl+Enter style).
                    let cmd = self
                        .suggest
                        .take()
                        .map(|s| s.matches[s.sel].to_string())
                        .unwrap_or_default();
                    if !cmd.is_empty() {
                        self.buf = cmd.clone();
                        self.cursor = cmd.chars().count();
                    }
                    let saved = self.enter_sends;
                    self.enter_sends = true;
                    let action = self.commit();
                    self.enter_sends = saved;
                    return action;
                }
                _ => {}
            }
        }

        // Shift+Enter: explicit newline in the input bar (chat-style); also
        // closes the suggestion popup since the buffer is no longer a plain
        // command line.
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Enter {
            self.insert_char('\n');
            self.multiline = true;
            self.suggest = None;
            return InputAction::None;
        }
        // Alt+Enter arrives as KeyCode::Enter with ALT on Windows terminals
        // (and as '\r' on others) — toggle multiline mode either way.
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Enter {
            return self.toggle_multiline();
        }

        // Multiline-mode send: Ctrl+Enter.
        if self.multiline
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Enter
        {
            return self.commit();
        }
        // Ctrl+Enter always sends under the "Ctrl+Enter sends" style (D30).
        if !self.enter_sends
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Enter
        {
            return self.commit();
        }

        let action = match key.code {
            KeyCode::Enter => {
                // Enter sends (D30 default); Shift+Enter is the newline key.
                // Only the "Ctrl+Enter sends" style makes Enter break lines.
                if !self.enter_sends {
                    self.insert_char('\n');
                    if !self.multiline {
                        self.multiline = true;
                    }
                    InputAction::None
                } else {
                    self.commit()
                }
            }
            KeyCode::Tab => {
                // Open the suggestion popup and fill the first match —
                // in either context: a slash-prefixed word (commands) or
                // the `/new ` prefix (agent-preset modes).
                let command_ctx = self.buf.starts_with('/') && !self.buf.contains([' ', '\n']);
                let mode_ctx = match self.buf.strip_prefix("/new ") {
                    Some(rest) => !rest.contains([' ', '\n']),
                    None => false,
                };
                if command_ctx || mode_ctx {
                    self.refresh_suggest();
                    if let Some(s) = self.suggest.as_mut() {
                        let cmd = s.matches[s.sel].clone();
                        self.buf = cmd.clone();
                        self.cursor = cmd.chars().count();
                    }
                }
                InputAction::None
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    // Alt+Enter toggles multiline.
                    if c == '\r' || c == '\n' {
                        return self.toggle_multiline();
                    }
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return InputAction::None;
                }
                self.insert_char(c);
                InputAction::None
            }
            KeyCode::Backspace => {
                if self.pasted {
                    // The cursor sits after the block; Backspace removes the
                    // whole placeholder content (design §4.1).
                    self.clear();
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buf.remove(char_to_byte(&self.buf, self.cursor));
                }
                InputAction::None
            }
            KeyCode::Delete => {
                if self.pasted {
                    // Cursor before the block: Delete removes the whole
                    // placeholder content.
                    self.clear();
                } else if self.cursor < self.buf.chars().count() {
                    self.buf.remove(char_to_byte(&self.buf, self.cursor));
                }
                InputAction::None
            }
            KeyCode::Left => {
                // The cursor skips the paste block as one unit.
                if self.pasted {
                    self.cursor = 0;
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                }
                InputAction::None
            }
            KeyCode::Right => {
                if self.pasted {
                    self.cursor = self.buf.chars().count();
                } else if self.cursor < self.buf.chars().count() {
                    self.cursor += 1;
                }
                InputAction::None
            }
            KeyCode::Home => {
                self.cursor = 0;
                InputAction::None
            }
            KeyCode::End => {
                self.cursor = self.buf.chars().count();
                InputAction::None
            }
            KeyCode::Up => {
                if self.multiline {
                    // Multiline: move between lines (history stays in
                    // single-line mode). Paste blocks stay atomic.
                    if !self.pasted {
                        self.cursor_up();
                    }
                } else {
                    self.history_prev();
                }
                InputAction::None
            }
            KeyCode::Down => {
                if self.multiline {
                    if !self.pasted {
                        self.cursor_down();
                    }
                } else {
                    self.history_next();
                }
                InputAction::None
            }
            KeyCode::Esc => {
                // Esc interrupts the current conversation (while the agent
                // runs); clearing the bar is Ctrl+C's job now.
                if idle {
                    InputAction::None
                } else {
                    InputAction::Interrupt
                }
            }
            _ => InputAction::None,
        };
        self.refresh_suggest();
        action
    }

    pub fn matching_history(&self, query: &str) -> Vec<String> {
        if query.is_empty() {
            return self.history.iter().rev().cloned().collect();
        }
        let q = query.to_lowercase();
        self.history
            .iter()
            .rev()
            .filter(|h| h.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Recompute the suggestion popup from the buffer. Two contexts feed it:
    /// a slash-prefixed word without a space lists slash commands, and the
    /// `/new ` prefix lists the agent-preset roster as new-session modes.
    /// While the user navigates (the buffer equals one of the listed rows)
    /// the list and its query stay pinned, so the highlight follows the
    /// filled value and Esc can still restore the typed query.
    fn refresh_suggest(&mut self) {
        if let Some(s) = &mut self.suggest {
            let buf_matches = s.matches.iter().position(|m| *m == self.buf);
            if s.query == self.buf || buf_matches.is_some() {
                if let Some(pos) = buf_matches {
                    s.sel = pos;
                }
                return;
            }
        }
        // Command-name popup: "/" or "/set…" without a space.
        let slash = self.buf.starts_with('/') && !self.buf.contains([' ', '\n']);
        if slash {
            let matched = match_commands(&self.buf[1..]);
            if matched.is_empty() {
                self.suggest = None;
                return;
            }
            let matches: Vec<String> = matched.iter().map(|cmd| (*cmd).to_string()).collect();
            let descriptions: Vec<String> = matched
                .iter()
                .map(|cmd| {
                    COMMANDS
                        .iter()
                        .find(|(c, _)| c == cmd)
                        .map(|(_, d)| (*d).to_string())
                        .unwrap_or_default()
                })
                .collect();
            self.suggest = Some(Suggestion {
                query: self.buf.clone(),
                sel: 0,
                matches,
                descriptions,
                modes: false,
            });
            return;
        }
        // `/new <mode>` popup: everything after the first space is the mode
        // query; a second space or a newline leaves the command line again.
        if let Some(rest) = self.buf.strip_prefix("/new ") {
            if !rest.contains([' ', '\n']) {
                let ranked = match_new_modes(rest, &self.new_modes);
                if ranked.is_empty() {
                    self.suggest = None;
                    return;
                }
                let matches: Vec<String> = ranked
                    .iter()
                    .map(|mode| format!("/new {}", mode.id))
                    .collect();
                let descriptions: Vec<String> = ranked
                    .iter()
                    .map(|mode| mode.name.clone().unwrap_or_else(|| mode.id.clone()))
                    .collect();
                self.suggest = Some(Suggestion {
                    query: self.buf.clone(),
                    sel: 0,
                    matches,
                    descriptions,
                    modes: true,
                });
                return;
            }
        }
        self.suggest = None;
    }

    fn insert_char(&mut self, c: char) {
        self.buf.insert(char_to_byte(&self.buf, self.cursor), c);
        self.cursor += 1;
        // The cursor may never rest inside a paste block: snap to the end.
        if self.pasted {
            self.cursor = self.buf.chars().count();
        }
    }

    /// Multiline: move the cursor to the end of the previous line.
    fn cursor_up(&mut self) {
        let byte = char_to_byte(&self.buf, self.cursor);
        let before = &self.buf[..byte];
        let Some(i) = before.rfind('\n') else {
            return; // already on the first line
        };
        // `i` is the byte index of the newline before the current line;
        // the previous line ends right there.
        self.cursor = self.buf[..i].chars().count();
    }

    /// Multiline: move the cursor to the end of the next line.
    fn cursor_down(&mut self) {
        let byte = char_to_byte(&self.buf, self.cursor);
        let after = &self.buf[byte..];
        let Some(i) = after.find('\n') else {
            return; // already on the last line
        };
        let next_start = byte + i + 1;
        let next_end = match self.buf[next_start..].find('\n') {
            Some(j) => next_start + j,
            None => self.buf.len(),
        };
        self.cursor = self.buf[..next_end].chars().count();
    }

    fn commit(&mut self) -> InputAction {
        let text = self.buf.trim_end_matches('\n').to_string();
        if text.is_empty() {
            self.suggest = None;
            return InputAction::None;
        }
        self.suggest = None;
        self.history.push(text.clone());
        if self.history.len() > self.history_limit {
            self.history.remove(0);
        }
        self.hist_idx = None;
        self.buf.clear();
        self.cursor = 0;
        self.pasted = false;
        if self.multiline {
            self.multiline = false;
            self.draft = None;
        }
        if text.starts_with('/') {
            InputAction::Command(text)
        } else {
            InputAction::Send(text)
        }
    }

    fn toggle_multiline(&mut self) -> InputAction {
        if !self.multiline {
            self.multiline = true;
        } else {
            self.multiline = false;
            self.draft = None;
        }
        InputAction::ToggleMultiline
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.hist_idx {
            None => self.history.len().saturating_sub(1),
            Some(0) => 0,
            Some(i) => i - 1,
        };
        if self.hist_idx.is_none() {
            self.draft = Some(self.buf.clone());
        }
        self.hist_idx = Some(idx);
        self.buf = self.history[idx].clone();
        self.cursor = self.buf.chars().count();
    }

    fn history_next(&mut self) {
        let Some(idx) = self.hist_idx else { return };
        if idx + 1 < self.history.len() {
            self.hist_idx = Some(idx + 1);
            self.buf = self.history[idx + 1].clone();
        } else {
            self.hist_idx = None;
            self.buf = self.draft.take().unwrap_or_default();
        }
        self.cursor = self.buf.chars().count();
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.suggest = None;
        self.pasted = false;
    }

    /// The buffer as shown: a paste block collapses into a colored
    /// placeholder (D24). Return the display text plus whether it is a
    /// placeholder.
    pub fn display_text(&self) -> (String, bool) {
        if self.pasted {
            (format!("[{} text pasted]", self.buf.chars().count()), true)
        } else {
            (self.buf.clone(), false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn state() -> InputState {
        let config = Config::default();
        InputState::new(&config)
    }

    #[test]
    fn history_search_filters_and_loads() {
        let mut s = state();
        s.history = vec!["hello".into(), "world".into(), "help me".into()];
        s.handle_key(&ctrl('r'), true);
        assert!(s.search.is_some());
        for c in "he".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(s.buf, "help me");
        assert!(s.search.is_none());
    }

    #[test]
    fn tab_completes_commands() {
        let mut s = state();
        for c in "/set".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s.buf, "/settings");
    }

    #[test]
    fn ctrl_c_clears_input_when_non_empty() {
        let mut s = state();
        for c in "abc".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(!s.buf.is_empty());
        let action = s.handle_key(&ctrl('c'), true);
        assert_eq!(action, InputAction::None);
        assert!(s.buf.is_empty(), "Ctrl+C clears the input bar");
    }

    #[test]
    fn ctrl_c_quits_only_when_idle_and_empty() {
        let mut s = state();
        // Running + empty bar: no-op (Esc interrupts instead).
        assert_eq!(s.handle_key(&ctrl('c'), false), InputAction::None);
        // Idle + empty bar: quit.
        assert_eq!(s.handle_key(&ctrl('c'), true), InputAction::Quit);
    }

    #[test]
    fn esc_interrupts_while_running() {
        let mut s = state();
        assert_eq!(
            s.handle_key(&key(KeyCode::Esc), false),
            InputAction::Interrupt
        );
        assert_eq!(s.handle_key(&key(KeyCode::Esc), true), InputAction::None);
    }

    #[test]
    fn exit_commands_are_slash_commands() {
        let mut s = state();
        for c in "/quit".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(action, InputAction::Command("/quit".into()));
        // All three variants fuzzy-match from "q".
        let m = match_commands("q");
        assert!(m.contains(&"/q") && m.contains(&"/quit"), "got: {m:?}");
    }

    #[test]
    fn slash_opens_full_command_list() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let suggest = s.suggest.as_ref().expect("popup opens on /");
        assert_eq!(suggest.matches.len(), COMMANDS.len());
        assert_eq!(suggest.query, "/");
        assert_eq!(suggest.sel, 0);
    }

    #[test]
    fn fuzzy_search_ranks_prefix_then_substring_then_fuzzy() {
        // Prefix match first.
        assert_eq!(match_commands("set")[0], "/settings");
        // Substring match.
        assert_eq!(match_commands("ett"), vec!["/settings"]);
        // Fuzzy subsequence (p-l-n inside "plan").
        assert_eq!(match_commands("pln"), vec!["/plan"]);
        // All prefix results come before everything else.
        let m = match_commands("c");
        assert!(m.iter().all(|c| c.starts_with("/c")), "prefix group only: {m:?}");
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn up_down_navigate_and_fill_buffer() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let list = s.suggest.as_ref().unwrap().matches.to_vec();
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, list[1], "Down fills the next command");
        assert_eq!(s.suggest.as_ref().unwrap().sel, 1);
        assert_eq!(s.suggest.as_ref().unwrap().matches.len(), list.len(), "list stays open");
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.buf, list[0]);
        s.handle_key(&key(KeyCode::Up), true); // wraps around
        assert_eq!(s.buf, list[list.len() - 1]);
        assert_eq!(s.cursor, s.buf.chars().count());
    }

    #[test]
    fn esc_restores_typed_query() {
        let mut s = state();
        for c in "/set".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_some());
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, "/settings");
        s.handle_key(&key(KeyCode::Esc), true);
        assert_eq!(s.buf, "/set", "Esc restores what was typed");
        assert!(s.suggest.is_none());
    }

    #[test]
    fn enter_sends_selected_command() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let list = s.suggest.as_ref().unwrap().matches.to_vec();
        s.handle_key(&key(KeyCode::Down), true);
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(action, InputAction::Command(list[1].to_string()));
        assert!(s.buf.is_empty());
        assert!(s.suggest.is_none());
    }

    #[test]
    fn enter_sends_highlight_even_without_navigation() {
        let mut s = state();
        for c in "/set".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(action, InputAction::Command("/settings".into()));
    }

    #[test]
    fn space_closes_suggestions() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        assert!(s.suggest.is_some());
        s.handle_key(&key(KeyCode::Char('s')), true);
        assert!(s.suggest.is_some());
        s.handle_key(&key(KeyCode::Char(' ')), true);
        assert!(s.suggest.is_none());
    }

    fn sample_modes() -> Vec<NewMode> {
        vec![
            NewMode {
                id: "standard".into(),
                name: Some("标准模式".into()),
                description: Some("功能完整".into()),
            },
            NewMode {
                id: "code".into(),
                name: Some("PTC 模式".into()),
                description: None,
            },
            NewMode {
                id: "minimal".into(),
                name: Some("极简模式".into()),
                description: Some("双工具".into()),
            },
            NewMode {
                id: "cordis".into(),
                name: Some("创造模式".into()),
                description: None,
            },
        ]
    }

    #[test]
    fn new_space_opens_mode_popup() {
        let mut s = state();
        s.new_modes = sample_modes();
        for c in "/new ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let suggest = s.suggest.as_ref().expect("mode popup opens after /new<space>");
        assert!(suggest.modes, "popup is the mode list");
        assert_eq!(suggest.matches.len(), 4, "empty query lists every mode");
        assert_eq!(suggest.matches[0], "/new standard", "roster order is kept");
        assert_eq!(suggest.descriptions[0], "标准模式");
        assert_eq!(suggest.query, "/new ");
        assert_eq!(suggest.sel, 0);
    }

    #[test]
    fn new_mode_query_filters_by_id_and_name() {
        let mut s = state();
        s.new_modes = sample_modes();
        for c in "/new m".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let suggest = s.suggest.as_ref().expect("popup stays for a partial mode");
        assert_eq!(suggest.matches, vec!["/new minimal"], "id prefix match");
        assert_eq!(suggest.descriptions, vec!["极简模式"]);
        // 创造模式 matches by its display name (substring), not its id.
        let mut s2 = state();
        s2.new_modes = sample_modes();
        for c in "/new 创造".chars() {
            s2.handle_key(&key(KeyCode::Char(c)), true);
        }
        let suggest2 = s2.suggest.as_ref().expect("name match opens the popup");
        assert_eq!(suggest2.matches, vec!["/new cordis"]);
    }

    #[test]
    fn new_mode_popup_closes_without_roster_or_on_second_space() {
        let mut s = state();
        for c in "/new ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(
            s.suggest.is_none(),
            "no popup until the bridge sends the presets roster"
        );
        s.new_modes = sample_modes();
        s.handle_key(&key(KeyCode::Char('m')), true);
        assert!(s.suggest.is_some());
        s.handle_key(&key(KeyCode::Char(' ')), true);
        assert!(s.suggest.is_none(), "a second space leaves the command line");
    }

    #[test]
    fn new_mode_navigation_and_enter_send_the_mode() {
        let mut s = state();
        s.new_modes = sample_modes();
        for c in "/new ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, "/new code", "Down fills the next mode");
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(action, InputAction::Command("/new code".into()));
        assert!(s.buf.is_empty());
        assert!(s.suggest.is_none());
    }

    #[test]
    fn new_mode_tab_cycles_and_esc_restores_query() {
        let mut s = state();
        s.new_modes = sample_modes();
        for c in "/new m".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_some());
        // Tab in the open popup advances to the next match (command-popup
        // semantics); with one match it re-fills the same row.
        s.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s.buf, "/new minimal");
        s.handle_key(&key(KeyCode::Esc), true);
        assert_eq!(s.buf, "/new m", "Esc restores the typed query");
        assert!(s.suggest.is_none());
    }

    #[test]
    fn new_tab_fills_first_mode_when_popup_is_closed() {
        let mut s = state();
        s.new_modes = sample_modes();
        for c in "/new ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_some(), "popup opens on the space");
        // The popup is open, so Tab cycles instead of filling the first
        // match; Esc first, then Tab opens a fresh popup on row 0.
        s.handle_key(&key(KeyCode::Esc), true);
        s.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s.buf, "/new standard", "Tab fills the first mode");
    }

    #[test]
    fn enter_sends_style_false_inserts_newline() {
        let mut s = state();
        s.enter_sends = false;
        s.handle_key(&key(KeyCode::Char('a')), true);
        s.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(s.buf, "a\n");
        assert!(s.multiline);
        // Ctrl+Enter sends even in multiline.
        let action = s.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            true,
        );
        assert!(matches!(action, InputAction::Send(_)));
    }

    #[test]
    fn shift_enter_inserts_newline() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), true);
        let action = s.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            true,
        );
        assert_eq!(action, InputAction::None, "Shift+Enter must not send");
        assert_eq!(s.buf, "a\n");
        assert!(s.multiline);
        // A second Shift+Enter keeps breaking lines.
        s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        assert_eq!(s.buf, "a\n\n");
        // Plain Enter still sends, even in multiline mode.
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert!(matches!(action, InputAction::Send(text) if text == "a"));
        assert!(!s.multiline);
        assert!(s.buf.is_empty());
    }

    #[test]
    fn enter_after_newline_sends_multiline_content() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), true);
        s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        s.handle_key(&key(KeyCode::Char('b')), true);
        // Enter sends the whole buffer, newline included (trailing ones trimmed).
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert!(matches!(action, InputAction::Send(text) if text == "a\nb"));
        assert!(!s.multiline);
    }

    #[test]
    fn shift_enter_closes_suggestion_and_breaks() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        assert!(s.suggest.is_some());
        let action = s.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            true,
        );
        assert_eq!(action, InputAction::None, "popup Enter must not fire on Shift+Enter");
        assert!(s.suggest.is_none());
        assert_eq!(s.buf, "/\n");
    }

    #[test]
    fn alt_enter_toggles_multiline() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), true);
        let action = s.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            true,
        );
        assert_eq!(action, InputAction::ToggleMultiline);
        assert!(s.multiline);
        let action = s.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            true,
        );
        assert_eq!(action, InputAction::ToggleMultiline);
        assert!(!s.multiline);
    }

    #[test]
    fn paste_over_threshold_becomes_block() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("123456");
        assert!(s.pasted, "paste over the threshold becomes a block");
        let (text, placeholder) = s.display_text();
        assert!(placeholder);
        assert_eq!(text, "[6 text pasted]");
        assert_eq!(s.cursor, 6, "cursor snapped to the end of the block");
        // A short paste stays ordinary text.
        let mut s2 = state();
        s2.paste_placeholder_chars = 5;
        s2.paste("12345");
        assert!(!s2.pasted);
        assert_eq!(s2.display_text().0, "12345");
    }

    /// The paste block is atomic: the cursor can never enter it, and
    /// Backspace removes the whole block.
    #[test]
    fn paste_block_cursor_is_atomic() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("123456");
        // Left from the end jumps to the very front.
        s.handle_key(&key(KeyCode::Left), true);
        assert_eq!(s.cursor, 0, "Left skips across the block");
        // Right from the front jumps to the very end.
        s.handle_key(&key(KeyCode::Right), true);
        assert_eq!(s.cursor, 6, "Right skips across the block");
        // Typing snaps the cursor back out of the block.
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&key(KeyCode::Char('x')), true);
        assert_eq!(s.cursor, 7, "cursor snaps to the block end after typing");
        // Backspace removes the whole placeholder content.
        s.handle_key(&key(KeyCode::Backspace), true);
        assert!(s.buf.is_empty());
        assert!(!s.pasted);
        // Delete before the block removes it too.
        s.paste("123456");
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&key(KeyCode::Delete), true);
        assert!(s.buf.is_empty());
        assert!(!s.pasted);
    }

    /// Typed text over the threshold stays ordinary (the block is for
    /// pastes only).
    #[test]
    fn typed_long_text_is_not_a_block() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        for c in "123456".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(!s.pasted);
        assert_eq!(s.display_text().0, "123456");
        assert_eq!(s.cursor, 6);
    }

    /// Shift+Enter multiline: Up/Down move between lines instead of walking
    /// history.
    #[test]
    fn multiline_up_down_move_between_lines() {
        let mut s = state();
        for c in "ab".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        for c in "cd".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(s.buf, "ab\ncd");
        assert_eq!(s.cursor, 5);
        // Up → end of the previous line.
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.cursor, 2);
        // Up again → no-op (already on the first line).
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.cursor, 2);
        // Down → end of the next line.
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.cursor, 5);
        // Down again → no-op (last line).
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.cursor, 5);
        // History is untouched in multiline mode.
        s.history = vec!["old".into()];
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.cursor, 2);
        assert_eq!(s.buf, "ab\ncd", "Up moves lines, not history");
    }

    /// Regression: inserting/removing CJK characters used to panic because
    /// `String::insert`/`remove` need byte indices while `cursor` is a char
    /// index. Typing the second multi-byte char crashed the program.
    #[test]
    fn cjk_insert_and_delete_are_char_boundary_safe() {
        let mut s = state();
        for c in "你好".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(s.buf, "你好");
        assert_eq!(s.cursor, 2);
        // Insert between the two chars.
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&key(KeyCode::Char('中')), true);
        assert_eq!(s.buf, "你中好");
        assert_eq!(s.cursor, 2);
        // Backspace removes the inserted char.
        s.handle_key(&key(KeyCode::Backspace), true);
        assert_eq!(s.buf, "你好");
        assert_eq!(s.cursor, 1);
        // Delete removes the char ahead.
        s.handle_key(&key(KeyCode::Delete), true);
        assert_eq!(s.buf, "你");
        assert_eq!(s.cursor, 1);
    }
}
