//! Low-level Input Page primitives.
//!
//! This leaf module owns focus navigation, text editing, viewport anchoring,
//! and generic page effects. Concrete settings/login/model/theme/resume pages
//! depend on it; it never imports those pages or their controllers.

use std::collections::HashMap;

use crate::key_mapping::{
    Action, MappedKey,
    MappedKey::{Command, Text},
};
#[cfg(test)]
use crate::key_mapping::{KeyMapping, Scope};
#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::AgentRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[cfg(test)]
pub fn direction_from_key(key: &KeyEvent) -> Option<Direction> {
    direction_from_input(KeyMapping::default().input(Scope::Page, key))
}

pub fn direction_from_input(key: MappedKey) -> Option<Direction> {
    match key {
        Command(Action::MoveLeft) => Some(Direction::Left),
        Command(Action::MoveDown | Action::NextOption) => Some(Direction::Down),
        Command(Action::MoveUp | Action::PreviousOption) => Some(Direction::Up),
        Command(Action::MoveRight) => Some(Direction::Right),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FocusId(pub String);

impl FocusId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone)]
pub struct FocusNode {
    pub id: FocusId,
    pub enabled: bool,
    pub left: Option<FocusId>,
    pub down: Option<FocusId>,
    pub up: Option<FocusId>,
    pub right: Option<FocusId>,
}

impl FocusNode {
    pub fn new(id: FocusId) -> Self {
        Self {
            id,
            enabled: true,
            left: None,
            down: None,
            up: None,
            right: None,
        }
    }

    fn neighbor(&self, direction: Direction) -> Option<&FocusId> {
        match direction {
            Direction::Left => self.left.as_ref(),
            Direction::Down => self.down.as_ref(),
            Direction::Up => self.up.as_ref(),
            Direction::Right => self.right.as_ref(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct FocusState {
    pub current: Option<FocusId>,
    nodes: HashMap<FocusId, FocusNode>,
    order: Vec<FocusId>,
}

impl FocusState {
    pub fn replace(&mut self, nodes: Vec<FocusNode>) {
        let previous = self.current.clone();
        self.order = nodes
            .iter()
            .filter(|node| node.enabled)
            .map(|node| node.id.clone())
            .collect();
        self.nodes = nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect();
        self.current = previous
            .filter(|id| self.nodes.get(id).is_some_and(|node| node.enabled))
            .or_else(|| self.order.first().cloned());
    }

    pub fn set(&mut self, id: FocusId) {
        if self.nodes.get(&id).is_some_and(|node| node.enabled) {
            self.current = Some(id);
        }
    }

    pub fn move_in(&mut self, direction: Direction) -> bool {
        let Some(current) = self.current.as_ref() else {
            self.current = self.order.first().cloned();
            return self.current.is_some();
        };
        let mut next = self
            .nodes
            .get(current)
            .and_then(|node| node.neighbor(direction))
            .cloned();
        for _ in 0..self.nodes.len() {
            let Some(candidate) = next else {
                return false;
            };
            let Some(node) = self.nodes.get(&candidate) else {
                return false;
            };
            if node.enabled {
                self.current = Some(candidate);
                return true;
            }
            next = node.neighbor(direction).cloned();
        }
        false
    }

    pub fn is(&self, id: &FocusId) -> bool {
        self.current.as_ref() == Some(id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEditor {
    pub buf: String,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEditResult {
    Continue,
    Confirm(String),
    Cancel,
}

#[cfg(test)]
pub fn handle_text_editor(editor: &mut TextEditor, key: &KeyEvent) -> TextEditResult {
    handle_text_input(editor, KeyMapping::default().input(Scope::PageEdit, key))
}

pub fn handle_text_input(editor: &mut TextEditor, key: MappedKey) -> TextEditResult {
    match key {
        Command(Action::Confirm) => TextEditResult::Confirm(std::mem::take(&mut editor.buf)),
        Command(Action::Cancel) => TextEditResult::Cancel,
        Command(Action::DeleteBackward) => {
            editor.buf.pop();
            TextEditResult::Continue
        }
        Text(character) => {
            editor.buf.push(character);
            TextEditResult::Continue
        }
        _ => TextEditResult::Continue,
    }
}

#[derive(Debug, Default, Clone)]
pub struct ViewportState {
    pub start: usize,
}

impl ViewportState {
    pub fn ensure_visible(&mut self, index: usize, visible: usize, total: usize) {
        if visible == 0 || total == 0 {
            self.start = 0;
            return;
        }
        if index < self.start {
            self.start = index;
        } else if index >= self.start.saturating_add(visible) {
            self.start = index.saturating_add(1).saturating_sub(visible);
        }
        self.start = self.start.min(total.saturating_sub(visible.min(total)));
    }
}

pub enum PageEffect {
    Send(AgentRequest),
    ConfigChanged,
}

#[derive(Default)]
pub struct PageOutcome {
    pub close: bool,
    pub effects: Vec<PageEffect>,
}

impl PageOutcome {
    pub(crate) fn close() -> Self {
        Self {
            close: true,
            effects: Vec::new(),
        }
    }

    pub(crate) fn send(message: AgentRequest, close: bool) -> Self {
        Self {
            close,
            effects: vec![PageEffect::Send(message)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn focus_reconciles_stable_ids_and_skips_disabled_nodes() {
        let a = FocusId::new("a");
        let b = FocusId::new("b");
        let c = FocusId::new("c");
        let mut first = FocusNode::new(a.clone());
        first.down = Some(b.clone());
        let mut disabled = FocusNode::new(b);
        disabled.enabled = false;
        disabled.down = Some(c.clone());
        let last = FocusNode::new(c.clone());
        let mut focus = FocusState::default();
        focus.replace(vec![first, disabled, last]);
        assert!(focus.move_in(Direction::Down));
        assert!(focus.is(&c));
        focus.replace(vec![FocusNode::new(c.clone()), FocusNode::new(a)]);
        assert!(focus.is(&c));
    }

    #[test]
    fn text_editor_keeps_vim_letters_as_text() {
        let mut editor = TextEditor {
            buf: String::new(),
            secret: false,
        };
        for character in "hjkl".chars() {
            assert_eq!(
                handle_text_editor(&mut editor, &key(KeyCode::Char(character))),
                TextEditResult::Continue
            );
        }
        assert_eq!(editor.buf, "hjkl");
    }

    #[test]
    fn text_editor_does_not_insert_modified_shortcut_letters() {
        let mut editor = TextEditor {
            buf: String::new(),
            secret: true,
        };
        assert_eq!(
            handle_text_editor(
                &mut editor,
                &KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
            ),
            TextEditResult::Continue
        );
        assert!(editor.buf.is_empty());
        handle_text_editor(&mut editor, &key(KeyCode::Char('v')));
        assert_eq!(editor.buf, "v");
    }

    #[test]
    fn modified_shortcuts_are_not_focus_directions() {
        assert_eq!(
            direction_from_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            direction_from_key(&key(KeyCode::Char('h'))),
            Some(Direction::Left)
        );
    }

    #[test]
    fn viewport_stays_bounded() {
        let mut viewport = ViewportState::default();
        viewport.ensure_visible(9, 3, 10);
        assert_eq!(viewport.start, 7);
        viewport.ensure_visible(1, 3, 10);
        assert_eq!(viewport.start, 1);
        viewport.ensure_visible(99, 0, 0);
        assert_eq!(viewport.start, 0);
    }
}
