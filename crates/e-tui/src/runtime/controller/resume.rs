use super::*;
use crate::{
    input_page::InputPage,
    resume::{ResumeBatch, ResumeRequest},
};

impl RuntimeController {
    pub fn take_resume_request(state: &Arc<Mutex<RuntimeState>>) -> Option<ResumeRequest> {
        let mut app = state.lock().unwrap();
        let workspace = app.session.session_cwd.clone()?;
        let session = app.interaction.input_page.as_mut()?;
        let InputPage::Resume(page) = &mut session.page else {
            return None;
        };
        page.take_request(&workspace)
    }

    pub fn apply_resume_batch(batch: ResumeBatch, state: &Arc<Mutex<RuntimeState>>) -> bool {
        let mut app = state.lock().unwrap();
        let Some(workspace) = app.session.session_cwd.clone() else {
            return false;
        };
        let Some(session) = app.interaction.input_page.as_mut() else {
            return false;
        };
        let InputPage::Resume(page) = &mut session.page else {
            return false;
        };
        page.apply_batch(batch, &workspace)
    }
}
