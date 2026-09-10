use super::*;
use crate::{
    agent::SessionSummary,
    input_page::InputPage,
    resume::{ResumeBatch, ResumeRequest},
};

fn app() -> Arc<Mutex<RuntimeState>> {
    let mut app = RuntimeState::default();
    app.session.session_cwd = Some("/project".into());
    app.interaction.input_page = Some(InputPageSession::resume());
    Arc::new(Mutex::new(app))
}

fn page(state: &Arc<Mutex<RuntimeState>>, f: impl FnOnce(&mut crate::input_page::ResumePage)) {
    let mut app = state.lock().unwrap();
    let InputPage::Resume(page) = &mut app.interaction.input_page.as_mut().unwrap().page else {
        panic!()
    };
    f(page);
}

fn batch(request: ResumeRequest, titles: &[&str], has_more: bool) -> ResumeBatch {
    assert!(titles.len() <= request.limit);
    assert!(request.limit <= 3);
    let sessions = titles
        .iter()
        .enumerate()
        .map(|(i, title)| SessionSummary {
            id: format!("/project/session-{}", request.offset + i),
            title: (*title).into(),
            live: false,
            created_at: 0,
            modified_at: None,
        })
        .collect();
    ResumeBatch {
        next_offset: request.offset + request.limit,
        request,
        sessions,
        has_more,
        diagnostic: None,
    }
}

#[test]
fn resume_demand_waits_for_geometry_then_prefetches_at_boundary() {
    let state = app();
    assert!(RuntimeController::take_resume_request(&state).is_none());
    page(&state, |page| page.paging.visible_rows = 2);
    let first = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(first.limit, 3);
    assert!(RuntimeController::take_resume_request(&state).is_none());
    assert!(RuntimeController::apply_resume_batch(
        batch(first, &["a", "b", "c"], true),
        &state
    ));
    page(&state, |page| {
        assert_eq!(page.sessions.len(), 3);
        assert!(!page.loading);
    });
    let remainder = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!((remainder.offset, remainder.limit), (3, 1));
    assert!(RuntimeController::apply_resume_batch(
        batch(remainder, &["d"], true),
        &state
    ));
    assert!(RuntimeController::take_resume_request(&state).is_none());
    page(&state, |page| page.sel = 2);
    let next = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(next.offset, 4);
    assert_eq!(next.limit, 3);
    assert!(RuntimeController::apply_resume_batch(
        batch(next, &["e", "f", "g"], true),
        &state
    ));
    page(&state, |page| {
        assert_eq!(page.sel, 2);
        assert_eq!(page.sessions[page.sel].title, "c");
        page.paging.visible_rows = 5;
    });
    let resized = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(resized.limit, 3);
}

#[test]
fn resume_search_scans_unloaded_rows_and_stops_when_cleared_or_exhausted() {
    let state = app();
    page(&state, |page| page.paging.visible_rows = 1);
    let first = RuntimeController::take_resume_request(&state).unwrap();
    RuntimeController::apply_resume_batch(batch(first, &["new", "recent"], true), &state);
    page(&state, |page| page.query = "old".into());
    let scan = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(scan.limit, 3);
    RuntimeController::apply_resume_batch(batch(scan, &["older", "oldest", "other"], true), &state);
    page(&state, |page| {
        assert_eq!(page.filtered_indices(), vec![2, 3]);
        assert!(page.search_pending());
        page.query.clear();
    });
    assert!(RuntimeController::take_resume_request(&state).is_none());
    page(&state, |page| page.query = "missing".into());
    let scan = RuntimeController::take_resume_request(&state).unwrap();
    RuntimeController::apply_resume_batch(batch(scan, &[], false), &state);
    page(&state, |page| {
        assert!(page.filtered_indices().is_empty());
        assert!(!page.search_pending());
    });
    assert!(RuntimeController::take_resume_request(&state).is_none());
}

#[test]
fn resume_rejects_closed_reopened_and_workspace_stale_results() {
    let state = app();
    page(&state, |page| page.paging.visible_rows = 1);
    let old = RuntimeController::take_resume_request(&state).unwrap();
    state.lock().unwrap().interaction.input_page = None;
    assert!(!RuntimeController::apply_resume_batch(
        batch(old.clone(), &["old"], true),
        &state
    ));
    state.lock().unwrap().interaction.input_page = Some(InputPageSession::resume());
    page(&state, |page| page.paging.visible_rows = 1);
    let new = RuntimeController::take_resume_request(&state).unwrap();
    assert_ne!(old.generation, new.generation);
    assert!(!RuntimeController::apply_resume_batch(
        batch(old, &["old"], true),
        &state
    ));
    state.lock().unwrap().session.session_cwd = Some("/other".into());
    assert!(!RuntimeController::apply_resume_batch(
        batch(new, &["old"], true),
        &state
    ));
    let other = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(other.workspace, "/other");
    assert_eq!(other.offset, 0);
    page(&state, |page| assert!(page.sessions.is_empty()));
}

#[test]
fn resume_age_refresh_is_page_scoped_and_consumes_each_deadline_once() {
    use std::time::Duration;

    let state = app();
    let due = Instant::now() + Duration::from_secs(1);
    page(&state, |page| page.age_refresh = Some(due));
    {
        let mut app = state.lock().unwrap();
        app.render.transcript_cache.valid = true;
        assert_eq!(app.reveal_deadline(), Some(due));
        assert!(!app.tick_reveals(due - Duration::from_millis(1)));
        assert!(app.tick_reveals(due));
        assert!(app.reveal_deadline().is_none());
        assert!(!app.tick_reveals(due));
        assert!(app.render.transcript_cache.valid);
    }
    page(&state, |page| {
        assert_eq!(page.sel, 0);
        page.age_refresh = Some(due);
    });
    let mut app = state.lock().unwrap();
    app.interaction.input_page = None;
    assert!(app.reveal_deadline().is_none());
    assert!(!app.tick_reveals(due));
}

#[test]
fn resume_skipped_batch_keeps_loading_until_a_valid_candidate_or_end() {
    let state = app();
    page(&state, |page| page.paging.visible_rows = 1);
    let first = RuntimeController::take_resume_request(&state).unwrap();
    RuntimeController::apply_resume_batch(batch(first, &[], true), &state);
    let next = RuntimeController::take_resume_request(&state).unwrap();
    assert_eq!(next.offset, 2);
    RuntimeController::apply_resume_batch(batch(next, &["valid"], false), &state);
    assert!(RuntimeController::take_resume_request(&state).is_none());
}
