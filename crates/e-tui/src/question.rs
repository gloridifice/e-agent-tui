//! Retained state for one blocking agent-question batch.

/// One pending user-question batch (ask_user_question), answered in an Input
/// Page (design §4.4). `current` is the visible question; `sel` and `draft`
/// mirror that question's retained option/text state while h/l navigates the
/// batch.
#[derive(Debug, Clone)]
pub struct QuestionBatch {
    pub rpc_id: String,
    pub session_id: String,
    pub questions: Vec<crate::agent::Question>,
    pub current: usize,
    pub sel: usize,
    pub draft: String,
    selections: Vec<usize>,
    chosen: Vec<Vec<usize>>,
    drafts: Vec<String>,
}

impl QuestionBatch {
    pub fn new(rpc_id: String, session_id: String, questions: Vec<crate::agent::Question>) -> Self {
        let count = questions.len();
        Self {
            rpc_id,
            session_id,
            questions,
            current: 0,
            sel: 0,
            draft: String::new(),
            selections: vec![0; count],
            chosen: vec![Vec::new(); count],
            drafts: vec![String::new(); count],
        }
    }

    /// Options of the question being answered (empty = free-text question).
    pub fn current_options(&self) -> &[crate::agent::QuestionOption] {
        self.questions
            .get(self.current)
            .and_then(|q| q.options.as_deref())
            .unwrap_or(&[])
    }

    pub fn is_free_text(&self) -> bool {
        self.current_options().is_empty()
    }

    /// j/k or ↓/↑: move the highlighted option (no-op for free text).
    pub fn step(&mut self, delta: isize) {
        let n = self.current_options().len();
        if n == 0 {
            return;
        }
        let next = if delta < 0 {
            self.sel.saturating_sub(delta.unsigned_abs())
        } else {
            (self.sel + delta as usize).min(n - 1)
        };
        self.sel = next;
        if let Some(selection) = self.selections.get_mut(self.current) {
            *selection = next;
        }
    }

    /// Space selects the focused option without changing questions. In a
    /// multi-select question it toggles that option; in a single-select
    /// question it replaces the previous explicit choice.
    pub fn toggle_selection(&mut self) {
        let Some(question) = self.questions.get(self.current) else {
            return;
        };
        let option_count = question
            .options
            .as_deref()
            .map_or(0, |options| options.len());
        if option_count == 0 || self.sel >= option_count {
            return;
        }
        let Some(chosen) = self.chosen.get_mut(self.current) else {
            return;
        };
        if question.multi_select {
            if let Some(index) = chosen.iter().position(|selected| *selected == self.sel) {
                chosen.remove(index);
            } else {
                chosen.push(self.sel);
                chosen.sort_unstable();
            }
        } else {
            chosen.clear();
            chosen.push(self.sel);
        }
    }

    pub fn is_option_selected(&self, option: usize) -> bool {
        self.chosen
            .get(self.current)
            .is_some_and(|chosen| chosen.contains(&option))
    }

    /// h/l or ←/→: move between questions while retaining each answer.
    pub fn step_question(&mut self, delta: isize) {
        if self.questions.is_empty() {
            return;
        }
        self.save_current();
        self.current = if delta < 0 {
            self.current.saturating_sub(delta.unsigned_abs())
        } else {
            (self.current + delta as usize).min(self.questions.len() - 1)
        };
        self.sel = self.selections.get(self.current).copied().unwrap_or(0);
        self.draft = self.drafts.get(self.current).cloned().unwrap_or_default();
    }

    fn save_current(&mut self) {
        if let Some(selection) = self.selections.get_mut(self.current) {
            *selection = self.sel;
        }
        if let Some(draft) = self.drafts.get_mut(self.current) {
            draft.clone_from(&self.draft);
        }
    }

    /// Printable key for the current free-text question.
    pub fn push_char(&mut self, c: char) {
        if self.is_free_text() {
            self.draft.push(c);
        }
    }

    /// Backspace for the current free-text question.
    pub fn backspace(&mut self) {
        if self.is_free_text() {
            self.draft.pop();
        }
    }

    /// Enter advances to the next question, or submits all retained answers
    /// from the last question.
    pub fn enter(&mut self) -> Option<Vec<crate::action::QuestionAnswer>> {
        if self.questions.get(self.current).is_none() {
            return Some(Vec::new());
        }
        self.save_current();
        if self.current + 1 < self.questions.len() {
            self.step_question(1);
            return None;
        }
        Some(
            self.questions
                .iter()
                .enumerate()
                .map(|(index, question)| self.answer_for(index, question))
                .collect(),
        )
    }

    /// Build the retained answer for one question from its saved state.
    fn answer_for(
        &self,
        index: usize,
        question: &crate::agent::Question,
    ) -> crate::action::QuestionAnswer {
        let options = question.options.as_deref().unwrap_or(&[]);
        let (selected, custom) = if options.is_empty() {
            let trimmed = self.drafts[index].trim().to_string();
            (Vec::new(), (!trimmed.is_empty()).then_some(trimmed))
        } else if question.multi_select {
            let selected = self.chosen[index]
                .iter()
                .filter_map(|selected| options.get(*selected))
                .map(|option| option.label.clone())
                .collect();
            (selected, None)
        } else {
            let selection = self.chosen[index]
                .first()
                .copied()
                .unwrap_or(self.selections[index])
                .min(options.len() - 1);
            (vec![options[selection].label.clone()], None)
        };
        crate::action::QuestionAnswer {
            id: question.id.clone(),
            selected,
            custom,
        }
    }
}
