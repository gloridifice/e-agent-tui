//! Event-driven frame admission shared by every executable adapter.

use std::time::{Duration, Instant};

pub const INTERACTIVE_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const CONTENT_FRAME_INTERVAL: Duration = Duration::from_millis(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyReason {
    Interactive,
    Content,
    Animation,
}

#[derive(Debug)]
pub struct FrameScheduler {
    deadline: Option<Instant>,
    requested_at: Option<Instant>,
    last_frame: Option<Instant>,
}

impl FrameScheduler {
    pub fn new(now: Instant) -> Self {
        Self {
            deadline: Some(now),
            requested_at: Some(now),
            last_frame: None,
        }
    }

    fn interval(reason: DirtyReason) -> Duration {
        match reason {
            DirtyReason::Interactive | DirtyReason::Animation => INTERACTIVE_FRAME_INTERVAL,
            DirtyReason::Content => CONTENT_FRAME_INTERVAL,
        }
    }

    pub fn request(&mut self, reason: DirtyReason, now: Instant) {
        let due = self
            .last_frame
            .map(|last| (last + Self::interval(reason)).max(now))
            .unwrap_or(now);
        self.deadline = Some(self.deadline.map_or(due, |current| current.min(due)));
        self.requested_at = Some(
            self.requested_at
                .map_or(now, |requested| requested.min(now)),
        );
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn take_due(&mut self, now: Instant) -> Option<Instant> {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            return self.requested_at.take();
        }
        None
    }

    pub fn complete(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_is_idle_after_due_frame_completes() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        assert_eq!(scheduler.take_due(now), Some(now));
        scheduler.complete(now);
        assert_eq!(scheduler.deadline(), None);
    }

    #[test]
    fn interactive_request_preempts_content_deadline() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        scheduler.take_due(now);
        scheduler.complete(now);
        scheduler.request(DirtyReason::Content, now);
        let content = scheduler.deadline().unwrap();
        scheduler.request(DirtyReason::Interactive, now);
        assert!(scheduler.deadline().unwrap() < content);
    }

    #[test]
    fn repeated_requests_coalesce_at_one_deadline() {
        let now = Instant::now();
        let mut scheduler = FrameScheduler::new(now);
        scheduler.take_due(now);
        scheduler.complete(now);
        scheduler.request(DirtyReason::Interactive, now);
        let due = scheduler.deadline();
        scheduler.request(DirtyReason::Interactive, now + Duration::from_millis(1));
        assert_eq!(scheduler.deadline(), due);
    }
}
