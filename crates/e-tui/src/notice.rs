//! Frontend-owned transient notices and their exact expiry deadlines.

use std::time::{Duration, Instant};

pub const COPY_NOTICE_MIN_SECS: u64 = 3;

#[derive(Debug, Clone, Default)]
pub struct NoticeState {
    current: Option<TransientNotice>,
}

#[derive(Debug, Clone)]
struct TransientNotice {
    text: String,
    shown_at: Instant,
}

impl NoticeState {
    pub fn show(&mut self, text: impl Into<String>, now: Instant) {
        self.current = Some(TransientNotice {
            text: text.into(),
            shown_at: now,
        });
    }

    pub fn show_clipboard(&mut self, lines: usize, preview: &str, truncated: bool, now: Instant) {
        let ellipsis = if truncated { "..." } else { "" };
        self.show(format!("已复制 {lines} 行：{preview}{ellipsis}"), now);
    }

    pub fn deadline(&self, configured_secs: u64) -> Option<Instant> {
        let duration = Duration::from_secs(configured_secs.max(COPY_NOTICE_MIN_SECS));
        self.current
            .as_ref()
            .map(|notice| notice.shown_at + duration)
    }

    pub fn visible_text(&self, configured_secs: u64, now: Instant) -> Option<&str> {
        self.current
            .as_ref()
            .filter(|_| {
                self.deadline(configured_secs)
                    .is_some_and(|deadline| deadline > now)
            })
            .map(|notice| notice.text.as_str())
    }

    pub fn expire(&mut self, configured_secs: u64, now: Instant) -> bool {
        if self
            .deadline(configured_secs)
            .is_none_or(|deadline| deadline > now)
        {
            return false;
        }
        self.current = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_notice_formats_and_honors_legacy_duration_floor() {
        let shown_at = Instant::now();
        let mut notice = NoticeState::default();
        notice.show_clipboard(2, "one tw", true, shown_at);
        assert_eq!(
            notice.visible_text(2, shown_at),
            Some("已复制 2 行：one tw...")
        );
        assert_eq!(notice.deadline(2), Some(shown_at + Duration::from_secs(3)));
        assert!(!notice.expire(2, shown_at + Duration::from_millis(2999)));
        assert!(notice.expire(2, shown_at + Duration::from_secs(3)));
    }
}
