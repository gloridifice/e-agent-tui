//! Input-line state and key handling (design §4.1, D23/D24).
//!
//! The input bar uses transparent horizontal rules around a text area (1 row,
//! or up to 5 scrolling rows in multiline mode). Each paste over the placeholder
//! threshold is an independent atomic
//! block that renders as `[N text pasted]` (like pi's paste markers); typed
//! text around blocks stays editable and the cursor skips blocks whole.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[cfg(test)]
use crate::agent::CommandDescriptor;
use crate::agent::{ModelDescriptor, ModelProvider, ReasoningEffort, Skill};
use crate::catalog::CatalogModel;
pub use crate::command_catalog::NewMode;
use crate::command_catalog::{
    candidate_description, completion_context, match_command_catalog, CommandSource, CompletionKind,
};
use crate::{
    action::{PromptImage, PromptInput, PromptPart},
    config::Config,
    i18n::{tr, tr_args, Language},
};

const IMAGE_MARKER: char = '\u{fffc}';
const IMAGE_NAME_DISPLAY_WIDTH: usize = 28;

#[derive(Clone)]
pub struct InputState {
    pub buf: String,
    /// Cursor as a char index into `buf`.
    pub cursor: usize,
    pub history: Vec<String>,
    pub hist_idx: Option<usize>,
    pub multiline: bool,
    /// Draft saved when entering multiline mode from a single line.
    pub draft: Option<String>,
    /// Atomic ranges belonging to the saved history-browsing draft.
    draft_paste_blocks: Vec<PasteBlock>,
    draft_image_blocks: Vec<ImageBlock>,
    /// Ctrl+R history search, when active.
    pub search: Option<SearchState>,
    /// Active language for localized stateful suggestions and placeholders.
    pub language: Language,
    /// Paste placeholder threshold (D24, config-driven).
    pub paste_placeholder_chars: usize,
    /// Over-threshold paste ranges in expanded-buffer character offsets.
    /// Each range renders as one placeholder and remains independently atomic.
    paste_blocks: Vec<PasteBlock>,
    /// Pending image attachments represented by one raw object marker and one
    /// atomic display block. Image bytes never enter `buf`.
    image_blocks: Vec<ImageBlock>,
    /// History cap (D28).
    pub history_limit: usize,
    // Characterization fixtures retain local catalogs only in test builds;
    // production obtains them from the sole `CatalogModel` owner.
    #[cfg(test)]
    pub new_modes: Vec<NewMode>,
    #[cfg(test)]
    pub integrated_commands: Vec<CommandDescriptor>,
    #[cfg(test)]
    pub skills: Vec<Skill>,
    /// Slash-command suggestion popup, when open.
    pub suggest: Option<Suggestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PasteBlock {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageBlock {
    start: usize,
    end: usize,
    image: PromptImage,
}

/// Display projection of the expanded input buffer. Large paste contents are
/// replaced by compact placeholders while the cursor is mapped to the same
/// logical boundary in display-character coordinates.
pub struct InputDisplay {
    pub text: String,
    pub cursor: usize,
    pub paste_ranges: Vec<Range<usize>>,
}

impl InputDisplay {
    pub fn is_paste_char(&self, index: usize) -> bool {
        self.paste_ranges.iter().any(|range| range.contains(&index))
    }
}

#[derive(Clone)]
pub struct SearchState {
    pub query: String,
    pub sel: usize,
}

/// Suggestion popup state: the ranked rows for the typed query, the
/// highlighted selection, and the raw query (kept for Esc restore while
/// the user navigates and the buffer shows the filled command).
#[derive(Clone)]
pub struct Suggestion {
    pub query: String,
    pub sel: usize,
    /// Ranked fill-in rows (what navigation fills and Enter sends):
    /// command lines like `/settings`, or `/new <mode-id>` lines.
    pub matches: Vec<String>,
    /// Per-row description column, parallel to `matches` (the command
    /// description or the mode's display name).
    pub descriptions: Vec<String>,
    /// Per-row source, parallel to `matches`: Builtin vs Integrated. Mode and
    /// skill rows are all Builtin (their popup headers identify the roster).
    pub sources: Vec<CommandSource>,
    pub kind: SuggestionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    Commands,
    Modes,
    Models,
    Efforts,
    Skills,
}

/// Normalize terminal and system-clipboard line endings to the frontend's
/// internal newline representation.
pub fn normalize_paste_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
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
            draft_paste_blocks: Vec::new(),
            draft_image_blocks: Vec::new(),
            search: None,
            language: config.language,
            paste_placeholder_chars: config.paste_placeholder_chars,
            paste_blocks: Vec::new(),
            image_blocks: Vec::new(),
            history_limit: config.history_limit,
            #[cfg(test)]
            new_modes: Vec::new(),
            #[cfg(test)]
            integrated_commands: Vec::new(),
            #[cfg(test)]
            skills: Vec::new(),
            suggest: None,
        }
    }

    /// Rebuild any open popup after the sole catalog owner changes.
    pub fn catalog_changed(&mut self, catalogs: &CatalogModel) {
        let selected = self
            .suggest
            .as_ref()
            .and_then(|suggest| suggest.matches.get(suggest.sel))
            .cloned();
        self.suggest = None;
        self.refresh_suggest(catalogs);
        if let Some(selected) = selected {
            if let Some(suggest) = self.suggest.as_mut() {
                if let Some(index) = suggest.matches.iter().position(|line| line == &selected) {
                    suggest.sel = index;
                }
            }
        }
    }

    #[cfg(test)]
    pub fn replace_integrated_commands(&mut self, commands: Vec<CommandDescriptor>) {
        self.integrated_commands = commands;
        let catalogs = self.test_catalogs();
        self.catalog_changed(&catalogs);
    }

    #[cfg(test)]
    pub fn replace_skills(&mut self, skills: Vec<Skill>) {
        self.skills = skills;
        let catalogs = self.test_catalogs();
        self.catalog_changed(&catalogs);
    }

    /// Insert pasted text at the cursor. Paste content over the threshold
    /// becomes an independent atomic block: surrounding typed text and other
    /// paste blocks remain editable, while cursor movement and deletion treat
    /// this range as one unit.
    pub fn paste(&mut self, text: &str) {
        let count = text.chars().count();
        if count == 0 {
            return;
        }
        let start = self.cursor;
        self.shift_blocks_for_insert(start, count);
        self.buf.insert_str(char_to_byte(&self.buf, start), text);
        self.cursor += count;
        if count > self.paste_placeholder_chars {
            self.paste_blocks.push(PasteBlock {
                start,
                end: self.cursor,
            });
            self.paste_blocks.sort_by_key(|block| block.start);
        }
        self.suggest = None;
    }

    /// Insert one pending image as an atomic object at the cursor.
    pub fn paste_image(&mut self, image: PromptImage) {
        let start = self.cursor;
        self.shift_blocks_for_insert(start, 1);
        self.buf
            .insert(char_to_byte(&self.buf, start), IMAGE_MARKER);
        self.cursor += 1;
        self.image_blocks.push(ImageBlock {
            start,
            end: self.cursor,
            image,
        });
        self.image_blocks.sort_by_key(|block| block.start);
        self.suggest = None;
    }

    /// Replace the whole composer with ordinary text from an external state
    /// restoration. Paste identity cannot be inferred from expanded text.
    pub fn restore_text(&mut self, text: String) {
        self.buf = text;
        self.cursor = self.buf.chars().count();
        self.paste_blocks.clear();
        self.image_blocks.clear();
        self.suggest = None;
    }

    /// Restore a complete prompt after deferred submission fails.
    pub fn restore_prompt(&mut self, prompt: PromptInput) {
        self.buf.clear();
        self.paste_blocks.clear();
        self.image_blocks.clear();
        for part in prompt.parts {
            match part {
                PromptPart::Text(text) => self.buf.push_str(&text),
                PromptPart::Image(image) => {
                    let start = self.buf.chars().count();
                    self.buf.push(IMAGE_MARKER);
                    self.image_blocks.push(ImageBlock {
                        start,
                        end: start + 1,
                        image,
                    });
                }
            }
        }
        self.cursor = self.buf.chars().count();
        self.suggest = None;
    }

    /// Fill the buffer with a plain command line while the suggestion popup
    /// stays open (popup navigation must not close itself).
    fn fill_text(&mut self, text: String) {
        self.buf = text;
        self.cursor = self.buf.chars().count();
        self.paste_blocks.clear();
        self.image_blocks.clear();
    }
}

#[derive(Debug, PartialEq)]
pub enum InputAction {
    None,
    /// Send the committed buffer as soon as the active turn can accept it.
    Send(PromptInput),
    /// Keep the committed buffer until the active turn has fully ended.
    SendAfterTurn(PromptInput),
    /// Send the committed buffer as a slash command line with any image inputs.
    Command {
        line: String,
        images: Vec<PromptImage>,
        original: PromptInput,
    },
    /// Interrupt the agent (Ctrl+C while running).
    Interrupt,
    /// Quit the TUI (Ctrl+C while idle).
    Quit,
    /// Toggle full-screen Preview on narrow terminals.
    PreviewToggle,
    /// Enter semantic Reading View (Ctrl+Y selected binding).
    ReadingToggle,
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

fn truncate_image_name(name: &str) -> String {
    if UnicodeWidthStr::width(name) <= IMAGE_NAME_DISPLAY_WIDTH {
        return name.to_owned();
    }
    let graphemes = name.graphemes(true).collect::<Vec<_>>();
    let suffix_budget = (IMAGE_NAME_DISPLAY_WIDTH / 3).max(6);
    let mut suffix = Vec::new();
    let mut suffix_width = 0;
    for grapheme in graphemes.iter().rev() {
        let width = UnicodeWidthStr::width(*grapheme);
        if suffix_width + width > suffix_budget {
            break;
        }
        suffix.push(*grapheme);
        suffix_width += width;
    }
    suffix.reverse();
    let head_budget = IMAGE_NAME_DISPLAY_WIDTH.saturating_sub(1 + suffix_width);
    let mut head = String::new();
    let mut head_width = 0;
    for grapheme in &graphemes {
        let width = UnicodeWidthStr::width(*grapheme);
        if head_width + width > head_budget {
            break;
        }
        head.push_str(grapheme);
        head_width += width;
    }
    format!("{head}…{}", suffix.concat())
}

fn image_placeholder(image: &PromptImage, language: Language) -> String {
    tr_args(
        language,
        "composer.image",
        &[(
            "name",
            truncate_image_name(image.name.as_deref().unwrap_or("clipboard.png")),
        )],
    )
}

/// Han ideographs and kana carry no space separation, so each grapheme is
/// its own word-deletion unit (VS Code behavior); Hangul uses spaces and
/// stays a regular word run. Script_Extensions covers halfwidth and extended
/// kana without maintaining scalar ranges by hand.
fn is_cjk_ish(grapheme: &str) -> bool {
    grapheme.chars().any(|c| {
        c.script_extension()
            .iter()
            .any(|script| matches!(script, Script::Han | Script::Hiragana | Script::Katakana))
    })
}

/// Word-deletion class: 0 word graphemes (letters, digits, `_`), 1 symbols,
/// 2 whitespace. A unit is a run of one non-whitespace class.
fn word_class(grapheme: &str) -> u8 {
    if grapheme.chars().all(char::is_whitespace) {
        2
    } else if grapheme
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        0
    } else {
        1
    }
}

/// Start index (char) of the grapheme-safe word-deletion unit ending at
/// `end`, where the preceding grapheme is already known to be non-whitespace.
fn word_unit_start(text: &str, end: usize) -> usize {
    let byte_end = char_to_byte(text, end);
    let mut char_start = 0;
    let units: Vec<(usize, u8, bool)> = text[..byte_end]
        .graphemes(true)
        .map(|grapheme| {
            let unit = (char_start, word_class(grapheme), is_cjk_ish(grapheme));
            char_start += grapheme.chars().count();
            unit
        })
        .collect();
    let &(mut start, class, cjk) = units
        .last()
        .expect("word_unit_start requires a non-empty prefix");
    if cjk {
        return start;
    }
    for &(candidate, candidate_class, candidate_cjk) in units[..units.len() - 1].iter().rev() {
        if candidate_cjk || candidate_class != class {
            break;
        }
        start = candidate;
    }
    start
}

/// Back up over whitespace without crossing `barrier` (a preceding paste-block
/// boundary). Returns the index of the first non-whitespace char, or `barrier`.
fn back_over_whitespace(chars: &[char], mut idx: usize, barrier: usize) -> usize {
    while idx > barrier && chars[idx - 1].is_whitespace() {
        idx -= 1;
    }
    idx
}

/// Built-in-only matching helper kept for callers/tests that do not own a
/// live bridge directory. Interactive completion uses `match_command_catalog`
/// below and therefore includes auto-discovered plugin commands.
pub fn match_commands(query: &str) -> Vec<String> {
    match_command_catalog(query, &[])
        .into_iter()
        .map(|candidate| candidate.line)
        .collect()
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

/// Fuzzy-rank model routes using the same searchable fields as Pi: model id,
/// provider id, canonical `provider/model`, and the display name. Completion
/// always fills the canonical route so duplicate model ids remain unambiguous.
pub fn match_models<'a>(
    query: &str,
    providers: &'a [ModelProvider],
) -> Vec<(&'a ModelProvider, &'a ModelDescriptor)> {
    let query = query.to_lowercase();
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    let mut fuzzy = Vec::new();
    for provider in providers {
        for model in &provider.models {
            let fields = [
                model.id.to_lowercase(),
                provider.id.to_lowercase(),
                format!("{}/{}", provider.id, model.id).to_lowercase(),
                model.name.to_lowercase(),
            ];
            if query.is_empty() || fields.iter().any(|field| field.starts_with(&query)) {
                prefix.push((provider, model));
            } else if fields.iter().any(|field| field.contains(&query)) {
                substring.push((provider, model));
            } else if fields.iter().any(|field| is_subsequence(&query, field)) {
                fuzzy.push((provider, model));
            }
        }
    }
    prefix.extend(substring);
    prefix.extend(fuzzy);
    prefix
}

/// Fuzzy-rank the exact current model route's effort ids and display names.
/// Completion always fills the provider-declared id and keeps roster order.
pub fn match_efforts<'a>(query: &str, efforts: &'a [ReasoningEffort]) -> Vec<&'a ReasoningEffort> {
    let query = query.to_lowercase();
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    let mut fuzzy = Vec::new();
    for effort in efforts {
        let fields = [effort.id.to_lowercase(), effort.name.to_lowercase()];
        if query.is_empty() || fields.iter().any(|field| field.starts_with(&query)) {
            prefix.push(effort);
        } else if fields.iter().any(|field| field.contains(&query)) {
            substring.push(effort);
        } else if fields.iter().any(|field| is_subsequence(&query, field)) {
            fuzzy.push(effort);
        }
    }
    prefix.extend(substring);
    prefix.extend(fuzzy);
    prefix
}

/// Fuzzy-rank skills by exact addressable name. The bridge already sends the
/// winning user-invocable roster sorted by name.
pub fn match_skills<'a>(query: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let q = query.to_lowercase();
    let mut prefix = Vec::new();
    let mut substring = Vec::new();
    let mut fuzzy = Vec::new();
    for skill in skills {
        let name = skill.name.to_lowercase();
        if q.is_empty() || name.starts_with(&q) {
            prefix.push(skill);
        } else if name.contains(&q) {
            substring.push(skill);
        } else if is_subsequence(&q, &name) {
            fuzzy.push(skill);
        }
    }
    prefix.extend(substring);
    prefix.extend(fuzzy);
    prefix
}

impl InputState {
    /// Compatibility helper for catalog-free callers and focused input tests.
    /// Production routes through `handle_key_with_catalog`.
    pub fn handle_key(&mut self, key: &KeyEvent, idle: bool) -> InputAction {
        #[cfg(test)]
        let catalogs = self.test_catalogs();
        #[cfg(not(test))]
        let catalogs = CatalogModel::default();
        self.handle_key_with_catalog(key, idle, &catalogs)
    }

    pub fn handle_key_with_catalog(
        &mut self,
        key: &KeyEvent,
        idle: bool,
        catalogs: &CatalogModel,
    ) -> InputAction {
        // Ctrl+C: clear the input bar; only an empty bar while idle quits.
        // (Esc is the interrupt key now.)
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if !self.buf.is_empty() {
                self.clear();
                self.search = None;
                self.multiline = false;
                self.draft = None;
                self.draft_paste_blocks.clear();
                self.draft_image_blocks.clear();
                return InputAction::None;
            }
            if idle {
                return InputAction::Quit;
            }
            return InputAction::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            return InputAction::PreviewToggle;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('y') {
            return InputAction::ReadingToggle;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
            return InputAction::None;
        }
        // Ctrl+R: reverse history search (design §4.1).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
            if !self.multiline {
                self.search = Some(SearchState {
                    query: String::new(),
                    sel: 0,
                });
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
                        self.restore_text(entry.clone());
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
                    self.restore_text(query);
                    return InputAction::None;
                }
                KeyCode::Up | KeyCode::Down => {
                    // At the popup boundary the arrow escapes the list and
                    // recalls the previous/next history prompt exactly like
                    // plain input; only inside the list does it move the
                    // selection (no wrap-around that would trap the user in
                    // the popup).
                    let at_edge = {
                        let s = self.suggest.as_mut().unwrap();
                        if key.code == KeyCode::Up {
                            if s.sel == 0 {
                                true
                            } else {
                                s.sel -= 1;
                                false
                            }
                        } else {
                            let n = s.matches.len();
                            if s.sel + 1 >= n {
                                true
                            } else {
                                s.sel += 1;
                                false
                            }
                        }
                    };
                    if at_edge {
                        self.suggest = None;
                        if key.code == KeyCode::Up {
                            self.history_prev();
                        } else {
                            self.history_next();
                        }
                        // Recompute the popup for the recalled prompt, the
                        // same way plain Up/Down refreshes after each key.
                        self.refresh_suggest(catalogs);
                        return InputAction::None;
                    }
                    let cmd = {
                        let s = self.suggest.as_ref().unwrap();
                        s.matches[s.sel].to_string()
                    };
                    // Auto-fill the selected command into the input bar.
                    self.fill_text(cmd.clone());
                    if cmd == "/skill" {
                        self.refresh_suggest(catalogs);
                    }
                    return InputAction::None;
                }
                KeyCode::Tab => {
                    // A partial buffer completes the highlighted row first;
                    // only a fully-typed row advances to the next candidate.
                    let s = self.suggest.as_mut().unwrap();
                    let n = s.matches.len();
                    let sel = s.sel;
                    if self.buf == s.matches[sel] {
                        s.sel = (sel + 1) % n;
                    }
                    let cmd = s.matches[s.sel].to_string();
                    // Auto-fill the selected command into the input bar.
                    self.fill_text(cmd.clone());
                    if cmd == "/skill" {
                        self.refresh_suggest(catalogs);
                    }
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
                        self.restore_text(cmd.clone());
                    }
                    return self.commit();
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

        // Ctrl+Enter deliberately queues ordinary messages until the current
        // turn has fully ended. Slash commands keep their ordinary command
        // behavior rather than entering the prompt queue.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
            return match self.commit() {
                InputAction::Send(prompt) => InputAction::SendAfterTurn(prompt),
                action => action,
            };
        }
        let action = match key.code {
            // Chat-style input is fixed: Enter sends; Shift+Enter above is
            // the only ordinary newline gesture.
            KeyCode::Enter => self.commit(),
            KeyCode::Tab => {
                // Open the suggestion popup and fill the first match —
                // in either context: a slash-prefixed word (commands) or
                // the `/new ` prefix (agent-preset modes).
                let command_ctx = self.buf.starts_with('/') && !self.buf.contains([' ', '\n']);
                let argument_ctx = completion_context(&self.buf)
                    .is_some_and(|(_, query)| !query.contains([' ', '\n']));
                if command_ctx || argument_ctx {
                    self.refresh_suggest(catalogs);
                    if let Some(s) = self.suggest.as_mut() {
                        let cmd = s.matches[s.sel].clone();
                        self.fill_text(cmd.clone());
                        if cmd == "/skill" {
                            self.refresh_suggest(catalogs);
                        }
                    }
                }
                InputAction::None
            }
            // Ctrl+W is the Unix delete-previous-word convention and is the
            // byte Windows Terminal emits for Ctrl+Backspace, so honor it here
            // even when the physical-key snapshot could not confirm the origin.
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.delete_word_back();
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
            // Ctrl+Backspace/Ctrl+W (Windows) and Alt/Option+Backspace
            // (macOS) delete the word before the cursor.
            KeyCode::Backspace
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.delete_word_back();
                InputAction::None
            }
            KeyCode::Backspace => {
                if let Some((start, end)) = self.block_ending_at(self.cursor) {
                    self.remove_range(start, end);
                } else if self.cursor > 0 {
                    let end = self.cursor;
                    self.remove_range(end - 1, end);
                }
                InputAction::None
            }
            KeyCode::Delete => {
                if let Some((start, end)) = self.block_starting_at(self.cursor) {
                    self.remove_range(start, end);
                } else if self.cursor < self.buf.chars().count() {
                    self.remove_range(self.cursor, self.cursor + 1);
                }
                InputAction::None
            }
            KeyCode::Left => {
                if let Some((start, _)) = self.block_ending_at(self.cursor) {
                    self.cursor = start;
                } else if self.cursor > 0 {
                    self.cursor -= 1;
                }
                InputAction::None
            }
            KeyCode::Right => {
                if let Some((_, end)) = self.block_starting_at(self.cursor) {
                    self.cursor = end;
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
                if !self.multiline || !self.cursor_up() {
                    self.history_prev();
                }
                InputAction::None
            }
            KeyCode::Down => {
                if !self.multiline || !self.cursor_down() {
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
        self.refresh_suggest(catalogs);
        action
    }

    #[cfg(test)]
    fn test_catalogs(&self) -> CatalogModel {
        CatalogModel {
            new_modes: self.new_modes.clone(),
            integrated_commands: self.integrated_commands.clone(),
            skills: self.skills.clone(),
            ..CatalogModel::default()
        }
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

    /// Recompute the suggestion popup from the buffer. A slash-prefixed word
    /// lists commands; declared argument contexts list modes, models, efforts,
    /// or user-invocable skills.
    /// While the user navigates (the buffer equals one of the listed rows)
    /// the list and its query stay pinned, so the highlight follows the
    /// filled value and Esc can still restore the typed query.
    fn refresh_suggest(&mut self, catalogs: &CatalogModel) {
        if !self.paste_blocks.is_empty() || !self.image_blocks.is_empty() {
            self.suggest = None;
            return;
        }
        if let Some(s) = &mut self.suggest {
            let buf_matches = s.matches.iter().position(|m| *m == self.buf);
            let entering_skill_roster = s.kind == SuggestionKind::Commands && self.buf == "/skill";
            if !entering_skill_roster && (s.query == self.buf || buf_matches.is_some()) {
                if let Some(pos) = buf_matches {
                    s.sel = pos;
                }
                return;
            }
        }
        // Argument popup selected by the built-in command's declaration. It
        // runs before command-name matching so exact `/skill` immediately
        // transitions from the command catalog to the skill roster.
        if let Some((command, query)) = completion_context(&self.buf) {
            if !query.contains([' ', '\n']) {
                match command.completion {
                    CompletionKind::NewMode => {
                        let ranked = match_new_modes(query, &catalogs.new_modes);
                        if ranked.is_empty() {
                            self.suggest = None;
                            return;
                        }
                        let matches: Vec<String> = ranked
                            .iter()
                            .map(|mode| format!("/{} {}", command.name, mode.id))
                            .collect();
                        let descriptions: Vec<String> = ranked
                            .iter()
                            .map(|mode| mode.name.clone().unwrap_or_else(|| mode.id.clone()))
                            .collect();
                        self.suggest = Some(Suggestion {
                            query: self.buf.clone(),
                            sel: 0,
                            sources: vec![CommandSource::Builtin; matches.len()],
                            matches,
                            descriptions,
                            kind: SuggestionKind::Modes,
                        });
                        return;
                    }
                    CompletionKind::Model => {
                        let ranked = match_models(query, &catalogs.model_providers);
                        if ranked.is_empty() {
                            self.suggest = None;
                            return;
                        }
                        let matches: Vec<String> = ranked
                            .iter()
                            .map(|(provider, model)| format!("/model {}/{}", provider.id, model.id))
                            .collect();
                        let descriptions: Vec<String> = ranked
                            .iter()
                            .map(|(provider, model)| {
                                if model.name == model.id {
                                    provider.name.clone()
                                } else {
                                    format!("{} · {}", model.name, provider.name)
                                }
                            })
                            .collect();
                        self.suggest = Some(Suggestion {
                            query: self.buf.clone(),
                            sel: 0,
                            sources: vec![CommandSource::Builtin; matches.len()],
                            matches,
                            descriptions,
                            kind: SuggestionKind::Models,
                        });
                        return;
                    }
                    CompletionKind::Effort => {
                        let ranked =
                            match_efforts(query, catalogs.current_efforts().unwrap_or_default());
                        if ranked.is_empty() {
                            self.suggest = None;
                            return;
                        }
                        let matches: Vec<String> = ranked
                            .iter()
                            .map(|effort| format!("/effort {}", effort.id))
                            .collect();
                        let descriptions =
                            ranked.iter().map(|effort| effort.name.clone()).collect();
                        self.suggest = Some(Suggestion {
                            query: self.buf.clone(),
                            sel: 0,
                            sources: vec![CommandSource::Builtin; matches.len()],
                            matches,
                            descriptions,
                            kind: SuggestionKind::Efforts,
                        });
                        return;
                    }
                    CompletionKind::Skill => {
                        let ranked = match_skills(query, &catalogs.skills);
                        if ranked.is_empty() {
                            self.suggest = None;
                            return;
                        }
                        let matches: Vec<String> = ranked
                            .iter()
                            .map(|skill| format!("/skill:{}", skill.name))
                            .collect();
                        let descriptions = ranked
                            .iter()
                            .map(|skill| skill.description.clone())
                            .collect();
                        self.suggest = Some(Suggestion {
                            query: self.buf.clone(),
                            sel: 0,
                            sources: vec![CommandSource::Builtin; matches.len()],
                            matches,
                            descriptions,
                            kind: SuggestionKind::Skills,
                        });
                        return;
                    }
                    CompletionKind::None => unreachable!("filtered by completion_context"),
                }
            }
        }
        // Command-name popup: "/" or "/set…" without a space.
        let slash = self.buf.starts_with('/') && !self.buf.contains([' ', '\n']);
        if slash {
            let matched = match_command_catalog(&self.buf[1..], &catalogs.integrated_commands);
            if matched.is_empty() {
                self.suggest = None;
                return;
            }
            self.suggest = Some(Suggestion {
                query: self.buf.clone(),
                sel: 0,
                matches: matched
                    .iter()
                    .map(|candidate| candidate.line.clone())
                    .collect(),
                descriptions: matched
                    .iter()
                    .map(|candidate| candidate_description(candidate, |key| tr(self.language, key)))
                    .collect(),
                sources: matched.iter().map(|candidate| candidate.source).collect(),
                kind: SuggestionKind::Commands,
            });
            return;
        }
        self.suggest = None;
    }

    fn insert_char(&mut self, c: char) {
        let at = self.cursor;
        self.shift_blocks_for_insert(at, 1);
        self.buf.insert(char_to_byte(&self.buf, at), c);
        self.cursor += 1;
    }

    fn shift_blocks_for_insert(&mut self, at: usize, count: usize) {
        for block in &mut self.paste_blocks {
            if block.start >= at {
                block.start += count;
                block.end += count;
            }
        }
        for block in &mut self.image_blocks {
            if block.start >= at {
                block.start += count;
                block.end += count;
            }
        }
    }

    fn block_ranges(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.paste_blocks
            .iter()
            .map(|block| (block.start, block.end))
            .chain(
                self.image_blocks
                    .iter()
                    .map(|block| (block.start, block.end)),
            )
    }

    fn block_starting_at(&self, cursor: usize) -> Option<(usize, usize)> {
        self.block_ranges().find(|(start, _)| *start == cursor)
    }

    fn block_ending_at(&self, cursor: usize) -> Option<(usize, usize)> {
        self.block_ranges().find(|(_, end)| *end == cursor)
    }

    fn preceding_block_end(&self, cursor: usize) -> usize {
        self.block_ranges()
            .map(|(_, end)| end)
            .filter(|&end| end <= cursor)
            .max()
            .unwrap_or(0)
    }

    fn remove_range(&mut self, start: usize, end: usize) {
        debug_assert!(start < end);
        let byte_start = char_to_byte(&self.buf, start);
        let byte_end = char_to_byte(&self.buf, end);
        self.buf.replace_range(byte_start..byte_end, "");
        let removed = end - start;
        // Drop any block the removal touched (fully or partially); a block
        // whose content was edited is no longer an atomic paste.
        self.paste_blocks
            .retain(|block| block.end <= start || block.start >= end);
        self.image_blocks
            .retain(|block| block.end <= start || block.start >= end);
        for block in &mut self.paste_blocks {
            if block.start >= end {
                block.start -= removed;
                block.end -= removed;
            }
        }
        for block in &mut self.image_blocks {
            if block.start >= end {
                block.start -= removed;
                block.end -= removed;
            }
        }
        self.cursor = start;
    }

    /// Ctrl+Backspace / Ctrl+W / Alt+Backspace: delete the word before the cursor
    /// together with the whitespace around it (Windows textbox style, so
    /// repeated presses clear the bar without leaving stray spaces). Paste
    /// blocks are atomic units; CJK ideographs and kana delete one grapheme
    /// at a time because they carry no spaces.
    fn delete_word_back(&mut self) {
        let end = self.cursor;
        if end == 0 {
            return;
        }
        let chars: Vec<char> = self.buf.chars().collect();
        // Check the cursor boundary before inspecting content: trailing
        // whitespace may belong to the atomic paste block itself.
        let unit_start = if let Some((start, _)) = self.block_ending_at(end) {
            start
        } else {
            let trailing_barrier = self.preceding_block_end(end);
            // Whitespace directly before the cursor goes with the word, but
            // the scan may reach, never enter, a preceding paste block.
            let start = back_over_whitespace(&chars, end, trailing_barrier);
            // A paste block ending at the word start is the unit; otherwise
            // the ordinary word scan stops at the nearest block boundary.
            match self.block_ending_at(start) {
                Some((block_start, _)) => block_start,
                None => {
                    if start == 0 {
                        // Only whitespace before the cursor.
                        self.remove_range(0, end);
                        return;
                    }
                    word_unit_start(&self.buf, start).max(trailing_barrier)
                }
            }
        };
        // The whitespace between the unit and whatever precedes it goes too,
        // without crossing into a previous atomic block that ends in space.
        let leading_barrier = self.preceding_block_end(unit_start);
        let final_start = back_over_whitespace(&chars, unit_start, leading_barrier);
        self.remove_range(final_start, end);
    }

    /// Move to the previous visual input line while preserving the character
    /// column where possible. Returns false at the first line so the caller
    /// can recall the previous prompt from history.
    fn cursor_up(&mut self) -> bool {
        let display = self.display_text();
        let byte = char_to_byte(&display.text, display.cursor);
        let current_start = display.text[..byte]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if current_start == 0 {
            return false;
        }
        let column = display.text[current_start..byte].chars().count();
        let previous_end = current_start - 1;
        let previous_start = display.text[..previous_end]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let previous_len = display.text[previous_start..previous_end].chars().count();
        let target_column = column.min(previous_len);
        let target = display.text[..previous_start].chars().count() + target_column;
        self.cursor = self.display_to_raw_cursor(target);
        true
    }

    /// Move to the next visual input line while preserving the character
    /// column where possible. Returns false at the last line so the caller
    /// can advance through prompt history or restore the draft.
    fn cursor_down(&mut self) -> bool {
        let display = self.display_text();
        let byte = char_to_byte(&display.text, display.cursor);
        let current_start = display.text[..byte]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let Some(relative_end) = display.text[byte..].find('\n') else {
            return false;
        };
        let column = display.text[current_start..byte].chars().count();
        let next_start = byte + relative_end + 1;
        let next_end = display.text[next_start..]
            .find('\n')
            .map_or(display.text.len(), |index| next_start + index);
        let next_len = display.text[next_start..next_end].chars().count();
        let target_column = column.min(next_len);
        let target = display.text[..next_start].chars().count() + target_column;
        self.cursor = self.display_to_raw_cursor(target);
        true
    }

    fn prompt(&self) -> PromptInput {
        let mut parts = Vec::new();
        let mut raw = 0usize;
        let raw_end = self.buf.trim_end_matches('\n').chars().count();
        let mut images = self.image_blocks.iter().collect::<Vec<_>>();
        images.sort_by_key(|block| block.start);
        for block in images {
            if block.start > raw_end {
                break;
            }
            let text = self
                .buf
                .chars()
                .skip(raw)
                .take(block.start.saturating_sub(raw))
                .collect::<String>();
            if !text.is_empty() {
                parts.push(PromptPart::Text(text));
            }
            parts.push(PromptPart::Image(block.image.clone()));
            raw = block.end;
        }
        let text = self
            .buf
            .chars()
            .skip(raw)
            .take(raw_end.saturating_sub(raw))
            .collect::<String>();
        if !text.is_empty() {
            parts.push(PromptPart::Text(text));
        }
        PromptInput { parts }
    }

    fn commit(&mut self) -> InputAction {
        let prompt = self.prompt();
        if prompt.is_empty() {
            self.suggest = None;
            return InputAction::None;
        }
        let command_line = prompt
            .parts
            .iter()
            .filter_map(|part| match part {
                PromptPart::Text(text) => Some(text.as_str()),
                PromptPart::Image(_) => None,
            })
            .collect::<String>();
        let images = prompt
            .parts
            .iter()
            .filter_map(|part| match part {
                PromptPart::Image(image) => Some(image.clone()),
                PromptPart::Text(_) => None,
            })
            .collect::<Vec<_>>();
        self.suggest = None;
        if images.is_empty() {
            self.history.push(command_line.clone());
            if self.history.len() > self.history_limit {
                self.history.remove(0);
            }
        }
        self.hist_idx = None;
        self.buf.clear();
        self.cursor = 0;
        self.paste_blocks.clear();
        self.image_blocks.clear();
        self.draft = None;
        self.draft_paste_blocks.clear();
        self.draft_image_blocks.clear();
        if self.multiline {
            self.multiline = false;
        }
        if command_line.starts_with('/') {
            InputAction::Command {
                line: command_line,
                images,
                original: prompt,
            }
        } else {
            InputAction::Send(prompt)
        }
    }

    fn toggle_multiline(&mut self) -> InputAction {
        if !self.multiline {
            self.multiline = true;
        } else {
            self.multiline = false;
            self.draft = None;
            self.draft_paste_blocks.clear();
            self.draft_image_blocks.clear();
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
            self.draft_paste_blocks = self.paste_blocks.clone();
            self.draft_image_blocks = self.image_blocks.clone();
        }
        self.hist_idx = Some(idx);
        self.buf = self.history[idx].clone();
        self.cursor = self.buf.chars().count();
        self.paste_blocks.clear();
        self.image_blocks.clear();
    }

    fn history_next(&mut self) {
        let Some(idx) = self.hist_idx else { return };
        if idx + 1 < self.history.len() {
            self.hist_idx = Some(idx + 1);
            self.buf = self.history[idx + 1].clone();
            self.paste_blocks.clear();
            self.image_blocks.clear();
        } else {
            self.hist_idx = None;
            self.buf = self.draft.take().unwrap_or_default();
            self.paste_blocks = std::mem::take(&mut self.draft_paste_blocks);
            self.image_blocks = std::mem::take(&mut self.draft_image_blocks);
        }
        self.cursor = self.buf.chars().count();
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
        self.suggest = None;
        self.paste_blocks.clear();
        self.image_blocks.clear();
    }

    /// The buffer as shown: each atomic paste block collapses into a
    /// placeholder (D24). The returned cursor is a character index into the
    /// returned display text; `paste_ranges` are display-character ranges of
    /// placeholder content.
    fn display_blocks(&self) -> Vec<(usize, usize, String)> {
        let mut blocks = self
            .paste_blocks
            .iter()
            .map(|block| {
                (
                    block.start,
                    block.end,
                    tr_args(
                        self.language,
                        "composer.paste",
                        &[("count", (block.end - block.start).to_string())],
                    ),
                )
            })
            .chain(self.image_blocks.iter().map(|block| {
                (
                    block.start,
                    block.end,
                    image_placeholder(&block.image, self.language),
                )
            }))
            .collect::<Vec<_>>();
        blocks.sort_by_key(|(start, _, _)| *start);
        blocks
    }

    pub fn display_text(&self) -> InputDisplay {
        let blocks = self.display_blocks();
        if blocks.is_empty() {
            return InputDisplay {
                text: self.buf.clone(),
                cursor: self.cursor,
                paste_ranges: Vec::new(),
            };
        }
        let raw_cursor = self.cursor;
        let mut text = String::with_capacity(self.buf.len());
        let mut paste_ranges = Vec::with_capacity(blocks.len());
        let mut cursor = raw_cursor;
        let mut raw = 0usize;
        let mut display_pos = 0usize;
        let mut mapped = false;
        for (start, end, placeholder) in blocks {
            let before = self.buf.chars().skip(raw).take(start - raw);
            for c in before {
                text.push(c);
            }
            let before_len = start - raw;
            if !mapped && raw_cursor <= start {
                cursor = display_pos + (raw_cursor - raw);
                mapped = true;
            }
            display_pos += before_len;
            let placeholder_len = placeholder.chars().count();
            if !mapped && raw_cursor <= end {
                cursor = display_pos + placeholder_len;
                mapped = true;
            }
            text.push_str(&placeholder);
            display_pos += placeholder_len;
            paste_ranges.push(display_pos - placeholder_len..display_pos);
            raw = end;
        }
        for c in self.buf.chars().skip(raw) {
            text.push(c);
        }
        if !mapped {
            cursor = display_pos + (raw_cursor - raw);
        }
        InputDisplay {
            text,
            cursor,
            paste_ranges,
        }
    }

    /// Map a display-character cursor (from `display_text`) back to the
    /// raw-buffer character index, keeping it out of atomic-block interiors.
    fn display_to_raw_cursor(&self, display_cursor: usize) -> usize {
        let blocks = self.display_blocks();
        if blocks.is_empty() {
            return display_cursor;
        }
        let mut raw = 0usize;
        let mut display_pos = 0usize;
        for (start, end, placeholder) in blocks {
            let before = start - raw;
            if display_cursor <= display_pos + before {
                return raw + (display_cursor - display_pos);
            }
            display_pos += before;
            let placeholder_len = placeholder.chars().count();
            if display_cursor < display_pos + placeholder_len {
                return start;
            }
            display_pos += placeholder_len;
            raw = end;
        }
        raw + (display_cursor - display_pos).min(self.buf.chars().count() - raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_catalog::BUILTIN_COMMANDS;
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

    fn image(name: &str) -> PromptImage {
        PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
            name: Some(name.into()),
        }
    }

    #[test]
    fn image_blocks_render_atomically_and_truncate_long_names() {
        let mut input = state();
        input.paste_image(image(
            "very-long-图片-capture-filename-that-keeps-going.png",
        ));
        let display = input.display_text();
        assert!(display.text.starts_with("[Image "));
        assert!(display.text.ends_with(".png]"));
        assert!(display.text.contains('…'));
        assert!(UnicodeWidthStr::width(display.text.as_str()) <= IMAGE_NAME_DISPLAY_WIDTH + 8);
        assert_eq!(input.buf, IMAGE_MARKER.to_string());

        input.handle_key(&key(KeyCode::Left), true);
        assert_eq!(input.cursor, 0, "Left skips the complete image block");
        input.handle_key(&key(KeyCode::Right), true);
        assert_eq!(input.cursor, 1, "Right skips the complete image block");
        input.handle_key(&key(KeyCode::Backspace), true);
        assert!(input.buf.is_empty());
        assert!(input.image_blocks.is_empty());

        input.paste_image(image("delete.png"));
        input.handle_key(&key(KeyCode::Left), true);
        input.handle_key(&key(KeyCode::Delete), true);
        assert!(input.buf.is_empty());
        assert!(input.image_blocks.is_empty());
    }

    #[test]
    fn mixed_and_image_only_commits_preserve_order_without_marker_text() {
        let mut input = state();
        input.paste("before ");
        let expected = image("clip.png");
        input.paste_image(expected.clone());
        input.paste(" after");
        let action = input.handle_key(&key(KeyCode::Enter), true);
        assert_eq!(
            action,
            InputAction::Send(PromptInput {
                parts: vec![
                    PromptPart::Text("before ".into()),
                    PromptPart::Image(expected.clone()),
                    PromptPart::Text(" after".into()),
                ],
            })
        );

        input.paste_image(expected.clone());
        assert_eq!(
            input.handle_key(&key(KeyCode::Enter), true),
            InputAction::Send(PromptInput {
                parts: vec![PromptPart::Image(expected.clone())],
            })
        );

        input.restore_prompt(PromptInput {
            parts: vec![PromptPart::Text("x".into()), PromptPart::Image(expected)],
        });
        assert_eq!(input.display_text().text, "x[Image clip.png]");
    }

    #[test]
    fn paste_text_normalizes_windows_line_endings() {
        assert_eq!(normalize_paste_text("a\r\nb\rc\n"), "a\nb\nc\n");
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
    fn ctrl_p_is_reserved_for_preview_toggle() {
        let mut s = state();
        assert_eq!(s.handle_key(&ctrl('p'), true), InputAction::PreviewToggle);
        assert_eq!(s.handle_key(&ctrl('y'), true), InputAction::ReadingToggle);
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
        assert!(
            matches!(action, InputAction::Command { line, images, .. } if line == "/quit" && images.is_empty())
        );
        // All three variants fuzzy-match from "q".
        let m = match_commands("q");
        assert!(
            m.iter().any(|item| item == "/q") && m.iter().any(|item| item == "/quit"),
            "got: {m:?}"
        );
    }

    #[test]
    fn slash_opens_full_command_list() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let suggest = s.suggest.as_ref().expect("popup opens on /");
        assert_eq!(suggest.matches.len(), BUILTIN_COMMANDS.len());
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
        assert!(
            m.iter().all(|c| c.starts_with("/c")),
            "prefix group only: {m:?}"
        );
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn integrated_plugin_command_is_auto_completed_with_its_hint() {
        let mut s = state();
        s.integrated_commands = vec![CommandDescriptor {
            name: "feedback".into(),
            description: "record feedback".into(),
            input_hint: Some("<text>".into()),
        }];
        for c in "/feed".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let suggest = s.suggest.as_ref().expect("plugin command is discovered");
        assert_eq!(suggest.matches, vec!["/feedback"]);
        assert!(suggest.descriptions[0].contains("<text>"));
    }

    #[test]
    fn live_directory_change_refreshes_an_open_popup() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        s.replace_integrated_commands(vec![CommandDescriptor {
            name: "feedback".into(),
            description: "record feedback".into(),
            input_hint: None,
        }]);
        assert!(s
            .suggest
            .as_ref()
            .unwrap()
            .matches
            .iter()
            .any(|item| item == "/feedback"));
    }

    #[test]
    fn up_down_navigate_and_fill_buffer() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let list = s.suggest.as_ref().unwrap().matches.to_vec();
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, list[1], "Down fills the next command");
        assert_eq!(s.suggest.as_ref().unwrap().sel, 1);
        assert_eq!(
            s.suggest.as_ref().unwrap().matches.len(),
            list.len(),
            "list stays open"
        );
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.buf, list[0]);
        assert_eq!(s.cursor, s.buf.chars().count());
    }

    #[test]
    fn up_at_popup_top_recalls_previous_prompt() {
        let mut s = state();
        s.history = vec!["a previous prompt".into()];
        for c in "/set".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_some());
        // At the top row, Up must escape the popup and recall the previous
        // prompt instead of wrapping around to the bottom row.
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.buf, "a previous prompt");
        assert!(s.suggest.is_none(), "popup closes at the boundary");
    }

    #[test]
    fn down_at_popup_bottom_restores_draft_instead_of_wrapping() {
        let mut s = state();
        s.history = vec!["/compact".into()];
        s.handle_key(&key(KeyCode::Char('/')), true);
        // Up at the top row recalls the history entry; the recalled
        // "/compact" reopens a one-row popup whose only row is also the
        // bottom of the list.
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.buf, "/compact");
        assert_eq!(s.suggest.as_ref().unwrap().matches.len(), 1);
        // Down on that bottom row must restore the draft ("/") — never
        // wrap around to the first row.
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, "/");
        assert!(s.hist_idx.is_none());
    }

    #[test]
    fn tab_completes_partial_row_before_advancing() {
        // User report: typing "/res" with the "/resume" candidate — the
        // first Tab must complete the highlighted row, not jump to the
        // next candidate in the list.
        let mut s = state();
        for c in "/res".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s.buf, "/resume", "first Tab completes the row");
        let sel = s.suggest.as_ref().unwrap().sel;
        assert_eq!(s.suggest.as_ref().unwrap().matches[sel], "/resume");
        // With several candidates the second Tab advances to the next row.
        let mut s2 = state();
        for c in "/r".chars() {
            s2.handle_key(&key(KeyCode::Char(c)), true);
        }
        s2.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s2.buf, "/read", "first Tab completes");
        s2.handle_key(&key(KeyCode::Tab), true);
        assert_eq!(s2.buf, "/reload", "second Tab advances to the next row");
        assert_eq!(s2.suggest.as_ref().unwrap().sel, 1);
    }

    #[test]
    fn esc_restores_typed_query() {
        let mut s = state();
        // "/r" matches several rows so Down stays inside the popup.
        for c in "/r".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_some());
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.buf, "/reload");
        s.handle_key(&key(KeyCode::Esc), true);
        assert_eq!(s.buf, "/r", "Esc restores what was typed");
        assert!(s.suggest.is_none());
    }

    #[test]
    fn enter_sends_selected_command() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('/')), true);
        let list = s.suggest.as_ref().unwrap().matches.to_vec();
        s.handle_key(&key(KeyCode::Down), true);
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert!(
            matches!(action, InputAction::Command { line, images, .. } if line == list[1] && images.is_empty())
        );
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
        assert!(
            matches!(action, InputAction::Command { line, images, .. } if line == "/settings" && images.is_empty())
        );
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

    fn sample_skills() -> Vec<Skill> {
        vec![
            Skill {
                name: "brooks-audit".into(),
                description: "Audit architecture".into(),
            },
            Skill {
                name: "code-review".into(),
                description: "Review a change".into(),
            },
            Skill {
                name: "imagegen".into(),
                description: "Generate an image".into(),
            },
        ]
    }

    #[test]
    fn skill_command_immediately_opens_skill_completion() {
        let mut s = state();
        s.skills = sample_skills();
        for c in "/skill".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        let suggest = s.suggest.as_ref().expect("skill roster opens at /skill");
        assert_eq!(suggest.kind, SuggestionKind::Skills);
        assert_eq!(
            suggest.matches,
            vec![
                "/skill:brooks-audit",
                "/skill:code-review",
                "/skill:imagegen"
            ]
        );
        assert_eq!(suggest.descriptions[0], "Audit architecture");
    }

    #[test]
    fn skill_completion_filters_colon_and_space_forms_to_canonical_colon() {
        let mut colon = state();
        colon.skills = sample_skills();
        for c in "/skill:aud".chars() {
            colon.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(
            colon.suggest.as_ref().unwrap().matches,
            vec!["/skill:brooks-audit"]
        );

        let mut space = state();
        space.skills = sample_skills();
        for c in "/skill image".chars() {
            space.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(
            space.suggest.as_ref().unwrap().matches,
            vec!["/skill:imagegen"]
        );
    }

    #[test]
    fn skill_directory_change_refreshes_an_open_popup() {
        let mut s = state();
        for c in "/skill".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert!(s.suggest.is_none());
        s.replace_skills(sample_skills());
        assert_eq!(s.suggest.as_ref().unwrap().kind, SuggestionKind::Skills);
    }

    fn sample_model_catalog() -> CatalogModel {
        CatalogModel {
            model_providers: vec![
                ModelProvider {
                    id: "anthropic".into(),
                    name: "Anthropic".into(),
                    models: vec![ModelDescriptor {
                        id: "claude-sonnet".into(),
                        name: "Claude Sonnet".into(),
                        description: None,
                        context_window: None,
                        reasoning: None,
                    }],
                },
                ModelProvider {
                    id: "openrouter".into(),
                    name: "OpenRouter".into(),
                    models: vec![ModelDescriptor {
                        id: "anthropic/claude-sonnet".into(),
                        name: "Claude Sonnet (OpenRouter)".into(),
                        description: None,
                        context_window: None,
                        reasoning: None,
                    }],
                },
            ],
            ..CatalogModel::default()
        }
    }

    #[test]
    fn model_space_opens_canonical_model_completion() {
        let mut input = state();
        let catalogs = sample_model_catalog();
        for character in "/model ".chars() {
            input.handle_key_with_catalog(&key(KeyCode::Char(character)), true, &catalogs);
        }

        let suggest = input
            .suggest
            .as_ref()
            .expect("model popup opens after /model<space>");
        assert_eq!(suggest.kind, SuggestionKind::Models);
        assert_eq!(
            suggest.matches,
            vec![
                "/model anthropic/claude-sonnet",
                "/model openrouter/anthropic/claude-sonnet"
            ]
        );
        assert_eq!(suggest.query, "/model ");
    }

    #[test]
    fn model_completion_fuzzy_matches_id_provider_and_name() {
        let catalogs = sample_model_catalog();
        for (query, expected) in [
            ("/model openr", "/model openrouter/anthropic/claude-sonnet"),
            (
                "/model (OpenRouter)",
                "/model openrouter/anthropic/claude-sonnet",
            ),
        ] {
            let mut input = state();
            for character in query.chars() {
                input.handle_key_with_catalog(&key(KeyCode::Char(character)), true, &catalogs);
            }
            assert_eq!(
                input.suggest.as_ref().unwrap().matches,
                vec![expected.to_owned()]
            );
        }
    }

    fn sample_effort_catalog() -> CatalogModel {
        CatalogModel {
            current_model: Some(crate::agent::ModelSelection {
                provider: "openai".into(),
                model: "gpt".into(),
                reasoning_effort: Some("medium".into()),
            }),
            model_providers: vec![ModelProvider {
                id: "openai".into(),
                name: "OpenAI".into(),
                models: vec![ModelDescriptor {
                    id: "gpt".into(),
                    name: "GPT".into(),
                    description: None,
                    context_window: None,
                    reasoning: Some(crate::agent::ModelReasoning {
                        efforts: vec![
                            ReasoningEffort {
                                id: "low".into(),
                                name: "Low".into(),
                                description: None,
                            },
                            ReasoningEffort {
                                id: "medium".into(),
                                name: "Balanced".into(),
                                description: None,
                            },
                            ReasoningEffort {
                                id: "xhigh".into(),
                                name: "Maximum".into(),
                                description: None,
                            },
                        ],
                        default_effort: Some("medium".into()),
                    }),
                }],
            }],
            ..CatalogModel::default()
        }
    }

    #[test]
    fn effort_space_opens_current_model_effort_completion() {
        let catalogs = sample_effort_catalog();
        let mut input = state();
        for character in "/effort ".chars() {
            input.handle_key_with_catalog(&key(KeyCode::Char(character)), true, &catalogs);
        }

        let suggest = input
            .suggest
            .as_ref()
            .expect("effort popup opens after /effort<space>");
        assert_eq!(suggest.kind, SuggestionKind::Efforts);
        assert_eq!(
            suggest.matches,
            vec!["/effort low", "/effort medium", "/effort xhigh"]
        );
        assert_eq!(suggest.descriptions, vec!["Low", "Balanced", "Maximum"]);
    }

    #[test]
    fn effort_completion_fuzzy_matches_id_and_display_name() {
        let catalogs = sample_effort_catalog();
        for (query, expected) in [
            ("/effort xh", "/effort xhigh"),
            ("/effort max", "/effort xhigh"),
        ] {
            let mut input = state();
            for character in query.chars() {
                input.handle_key_with_catalog(&key(KeyCode::Char(character)), true, &catalogs);
            }
            assert_eq!(
                input.suggest.as_ref().unwrap().matches,
                vec![expected.to_owned()]
            );
        }
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
        let suggest = s
            .suggest
            .as_ref()
            .expect("mode popup opens after /new<space>");
        assert_eq!(suggest.kind, SuggestionKind::Modes);
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
        assert!(
            s.suggest.is_none(),
            "a second space leaves the command line"
        );
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
        assert!(
            matches!(action, InputAction::Command { line, images, .. } if line == "/new code" && images.is_empty())
        );
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
        // Tab on a partial buffer completes the highlighted row (with a
        // single match that fills "/new minimal"); a second Tab would
        // cycle the one-row list in place.
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
    fn enter_always_sends_even_with_legacy_config_false() {
        let mut config = Config::default();
        config.enter_sends = false;
        let mut s = InputState::new(&config);
        s.handle_key(&key(KeyCode::Char('a')), true);
        assert_eq!(
            s.handle_key(&key(KeyCode::Enter), true),
            InputAction::Send("a".into())
        );
        assert!(s.buf.is_empty());
    }

    #[test]
    fn ctrl_enter_marks_an_ordinary_prompt_for_after_turn_delivery() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), false);
        assert_eq!(
            s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), false,),
            InputAction::SendAfterTurn("a".into())
        );
    }

    #[test]
    fn shift_enter_inserts_newline() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), true);
        let action = s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
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
        let action = s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        assert_eq!(
            action,
            InputAction::None,
            "popup Enter must not fire on Shift+Enter"
        );
        assert!(s.suggest.is_none());
        assert_eq!(s.buf, "/\n");
    }

    #[test]
    fn alt_enter_toggles_multiline() {
        let mut s = state();
        s.handle_key(&key(KeyCode::Char('a')), true);
        let action = s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        assert_eq!(action, InputAction::ToggleMultiline);
        assert!(s.multiline);
        let action = s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        assert_eq!(action, InputAction::ToggleMultiline);
        assert!(!s.multiline);
    }

    #[test]
    fn paste_over_threshold_becomes_block() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("123456");
        assert_eq!(
            s.paste_blocks.len(),
            1,
            "paste over the threshold becomes a block"
        );
        let display = s.display_text();
        assert_eq!(display.text, "[6 text pasted]");
        assert_eq!(display.paste_ranges.len(), 1);
        assert_eq!(display.cursor, display.text.chars().count());
        assert_eq!(s.cursor, 6, "cursor sits at the end of the block");
        // A short paste stays ordinary text.
        let mut s2 = state();
        s2.paste_placeholder_chars = 5;
        s2.paste("12345");
        assert!(s2.paste_blocks.is_empty());
        assert_eq!(s2.display_text().text, "12345");
    }

    /// A paste block between typed text stays atomic: the cursor skips it
    /// as one unit, Backspace/Delete remove the whole block, and surrounding
    /// text remains editable.
    #[test]
    fn paste_block_cursor_is_atomic() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        for c in "ab".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.paste("123456");
        for c in "cd".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(s.buf, "ab123456cd");
        // Left walks up to the block boundary, then skips it whole.
        s.handle_key(&key(KeyCode::Left), true);
        assert_eq!(s.cursor, 9, "Left moves before 'd'");
        s.handle_key(&key(KeyCode::Left), true);
        assert_eq!(s.cursor, 8, "Left stops at the block end");
        s.handle_key(&key(KeyCode::Left), true);
        assert_eq!(s.cursor, 2, "Left skips across the block");
        // Right from the front jumps to the very end of the block.
        s.handle_key(&key(KeyCode::Right), true);
        assert_eq!(s.cursor, 8, "Right skips across the block");
        // Backspace removes the whole block, keeping surrounding text.
        s.handle_key(&key(KeyCode::Backspace), true);
        assert_eq!(s.buf, "abcd");
        assert!(s.paste_blocks.is_empty());
        assert_eq!(s.cursor, 2);
        // Delete before the block removes it too.
        s.paste("123456");
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&key(KeyCode::Delete), true);
        assert_eq!(s.buf, "abcd");
        assert!(s.paste_blocks.is_empty());
    }

    /// Ctrl+Backspace/Ctrl+W deletes the word before the cursor together with the
    /// whitespace between it and the cursor (Windows textbox style).
    #[test]
    fn ctrl_backspace_deletes_word_with_whitespace() {
        let ctrl_bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL);
        let mut s = state();
        for c in "hello world".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "hello");
        assert_eq!(s.cursor, 5);
        // A second press clears the bar without leaving stray spaces.
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        // Multiple spaces collapse in one press.
        let mut s = state();
        for c in "a   b".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "a");
        // Trailing whitespace takes the word before it with it.
        let mut s = state();
        for c in "a   ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        // Mid-word deletes only the part before the cursor.
        let mut s = state();
        for c in "hello".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&key(KeyCode::Left), true);
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "lo");
        assert_eq!(s.cursor, 0);
    }

    /// CJK ideographs and kana delete one grapheme per press; Latin runs stay
    /// whole words; symbol runs are their own unit.
    #[test]
    fn ctrl_backspace_word_units() {
        let ctrl_bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL);
        // CJK: one grapheme per press.
        let mut s = state();
        for c in "你好世界".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "你好世");
        // Mixed: the CJK grapheme is its own unit, then the Latin word.
        let mut s = state();
        for c in "hello你好".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "hello你");
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "hello");
        // snake_case is one word (underscore is a word character).
        let mut s = state();
        for c in "foo_bar".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        // A symbol run is one unit; its separating space goes with it.
        let mut s = state();
        for c in "foo ...".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "foo");
        // Combining marks remain attached to their base word.
        let mut s = state();
        for c in "cafe\u{301}".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        // Script_Extensions recognizes halfwidth kana outside the old ranges.
        let mut s = state();
        for c in "ｶﾅ".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "ｶ");
        // Supplementary kana and current Han extensions use the same property
        // path instead of depending on manually updated scalar ranges.
        let mut s = state();
        for c in "\u{1aff0}\u{1aff1}".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "\u{1aff0}");
        let mut s = state();
        for c in "\u{2ebf0}\u{2ebf1}".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "\u{2ebf0}");
    }

    /// Alt/Option+Backspace (the macOS delete-word gesture) shares the
    /// Ctrl+Backspace path.
    #[test]
    fn alt_backspace_deletes_word() {
        let mut s = state();
        for c in "hello world".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT), true);
        assert_eq!(s.buf, "hello");
    }

    /// Windows Terminal sends Ctrl+W's byte for Ctrl+Backspace, so Ctrl+W
    /// shares the delete-word path.
    #[test]
    fn ctrl_w_deletes_word() {
        let mut s = state();
        for c in "hello world".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(
            &KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            true,
        );
        assert_eq!(s.buf, "hello");
        // Plain 'w' still types normally.
        s.handle_key(&key(KeyCode::Char('w')), true);
        assert_eq!(s.buf, "hellow");
    }

    /// Ctrl+Backspace treats a paste block as one atomic word unit.
    #[test]
    fn ctrl_backspace_deletes_paste_block_whole() {
        let ctrl_bs = KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL);
        let mut s = state();
        s.paste_placeholder_chars = 5;
        for c in "ab".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.paste("123456");
        for c in "cd".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        // "cd" goes first as an ordinary word.
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "ab123456");
        assert_eq!(s.paste_blocks.len(), 1);
        // The block is the next unit and is removed whole.
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "ab");
        assert!(s.paste_blocks.is_empty());
        assert_eq!(s.cursor, 2);
        // Whitespace after a block goes with the block.
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("123456");
        s.handle_key(&key(KeyCode::Char(' ')), true);
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        assert!(s.paste_blocks.is_empty());
        // Whitespace inside the block cannot let the scan partially enter it.
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("foo bar ");
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        assert!(s.paste_blocks.is_empty());
        // A following typed word is deleted without consuming trailing
        // whitespace that belongs to the preceding block.
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("123456 ");
        for c in "word".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "123456 ");
        assert_eq!(s.paste_blocks, vec![PasteBlock { start: 0, end: 7 }]);
        s.handle_key(&ctrl_bs, true);
        assert_eq!(s.buf, "");
        assert!(s.paste_blocks.is_empty());
    }

    /// Two paste blocks are independently atomic; deleting one leaves the
    /// other's range shifted correctly.
    #[test]
    fn multiple_paste_blocks_are_independent() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("AAAAAA");
        for c in "xy".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.paste("BBBBBB");
        assert_eq!(s.buf, "AAAAAAxyBBBBBB");
        assert_eq!(s.paste_blocks.len(), 2);
        let display = s.display_text();
        assert_eq!(display.text, "[6 text pasted]xy[6 text pasted]");
        assert_eq!(display.paste_ranges.len(), 2);
        // Backspace at the end removes only the second block.
        s.handle_key(&key(KeyCode::Backspace), true);
        assert_eq!(s.buf, "AAAAAAxy");
        assert_eq!(s.paste_blocks.len(), 1);
        assert_eq!(s.paste_blocks[0].end, 6);
        // Enter sends the full expanded content verbatim.
        let action = s.handle_key(&key(KeyCode::Enter), true);
        assert!(matches!(action, InputAction::Send(text) if text == "AAAAAAxy"));
    }

    #[test]
    fn multiple_paste_blocks_map_cursor_to_placeholder_boundaries() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        s.paste("AAAAAA");
        for c in "xy".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.paste("BBBBBB");
        let expected = "[6 text pasted]xy[6 text pasted]";

        // Cursor after the first block sits at the first placeholder end, not
        // inside a later placeholder.
        s.cursor = 6;
        let display = s.display_text();
        assert_eq!(display.text, expected);
        assert_eq!(display.cursor, "[6 text pasted]".chars().count());

        // Cursor before the second block sits at the second placeholder start.
        s.cursor = 8;
        let display = s.display_text();
        assert_eq!(display.text, expected);
        assert_eq!(display.cursor, "[6 text pasted]xy".chars().count());

        // A cursor forced into a block still snaps to that block's placeholder end.
        s.cursor = 3;
        let display = s.display_text();
        assert_eq!(display.text, expected);
        assert_eq!(display.cursor, "[6 text pasted]".chars().count());
    }

    /// A paste inside multiline content keeps Up/Down line movement working:
    /// the cursor maps through the placeholder and never rests inside it.
    #[test]
    fn multiline_navigation_maps_through_paste_block() {
        let mut s = state();
        s.paste_placeholder_chars = 5;
        for c in "ab".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        s.paste("123456");
        s.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), true);
        for c in "cd".chars() {
            s.handle_key(&key(KeyCode::Char(c)), true);
        }
        assert_eq!(s.buf, "ab\n123456\ncd");
        assert_eq!(s.cursor, 12);
        // Up from the last line lands on the placeholder line, snapped to
        // the block start (pi's snap-to-marker-start behavior).
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.cursor, 3, "cursor snaps to the block start");
        // Up again reaches the first line.
        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.cursor, 0);
        // Down maps back onto the placeholder line without entering it.
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.cursor, 3);
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(s.cursor, 10);
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
        assert!(s.paste_blocks.is_empty());
        assert_eq!(s.display_text().text, "123456");
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

    #[test]
    fn multiline_boundaries_switch_prompt_history() {
        let mut s = state();
        s.history = vec!["older".into(), "newer".into()];
        s.buf = "ab\ncd".into();
        s.cursor = 1;
        s.multiline = true;

        s.handle_key(&key(KeyCode::Up), true);
        assert_eq!(s.buf, "newer", "Up on the first line recalls history");
        s.handle_key(&key(KeyCode::Down), true);
        assert_eq!(
            s.buf, "ab\ncd",
            "Down at the history end restores the draft"
        );
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
