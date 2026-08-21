//! Presentation-only paced text reveal and foreground fading.
//!
//! Semantic transcript/Preview content stays complete. Transcript lanes pace
//! an admitted rendered-grapheme prefix; Preview lanes pace wrapped display
//! rows. Foreground fade groups have an independent frame clock.

use std::time::{Duration, Instant};

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;

/// Blend fraction for the newest fade group (age 0): a freshly revealed
/// grapheme renders this far from the background toward full foreground.
const NEWEST_FADE_FRACTION: f64 = 0.217;
/// Blend fraction for the next fade group (age 1) before full foreground.
const OLDER_FADE_FRACTION: f64 = 0.53;
/// Foreground contribution for the newest through oldest fade ages.
pub static TEXT_FADE_WEIGHTS: &[f64] = &[NEWEST_FADE_FRACTION, OLDER_FADE_FRACTION];

const MIN_REVEAL_FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const STREAM_IDLE_HOLD: Duration = Duration::from_millis(100);
pub const STREAM_MAX_HOLD: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RevealSignature {
    /// All graphemes concatenated into one shared buffer. Lines are segmented
    /// independently, so a cluster never spans a rendered line boundary.
    text: String,
    /// Absolute byte offset of each grapheme start, in reveal order.
    grapheme_starts: Vec<usize>,
}

impl RevealSignature {
    pub fn from_lines(lines: &[Line<'static>]) -> Self {
        Self::from_line_iter(lines.iter())
    }

    /// Build a signature from already-rendered lines without cloning them.
    pub fn from_line_iter<'a>(lines: impl IntoIterator<Item = &'a Line<'static>>) -> Self {
        let mut text = String::new();
        let mut grapheme_starts = Vec::new();
        for line in lines {
            let line_start = text.len();
            for span in &line.spans {
                text.push_str(span.content.as_ref());
            }
            grapheme_starts.extend(
                text[line_start..]
                    .grapheme_indices(true)
                    .map(|(offset, _)| line_start + offset),
            );
        }
        Self {
            text,
            grapheme_starts,
        }
    }

    pub fn grapheme_count(&self) -> usize {
        self.grapheme_starts.len()
    }

    fn grapheme(&self, index: usize) -> &str {
        let start = self.grapheme_starts[index];
        let end = self
            .grapheme_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        &self.text[start..end]
    }
}

#[derive(Debug, Clone)]
struct FadeGroup {
    start: usize,
    end: usize,
    age: usize,
}

/// Earliest of two optional deadlines, shared by both reveal lanes and the
/// main-loop animation clock.
pub fn earliest_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn age_fade_groups(groups: &mut Vec<FadeGroup>) -> bool {
    if groups.is_empty() {
        return false;
    }
    for group in groups.iter_mut() {
        group.age = group.age.saturating_add(1);
    }
    groups.retain(|group| group.age < TEXT_FADE_WEIGHTS.len());
    true
}

fn truncate_fade_groups(groups: &mut Vec<FadeGroup>, revealed: usize) {
    groups.retain_mut(|group| {
        if group.start >= revealed {
            return false;
        }
        group.end = group.end.min(revealed);
        group.start < group.end
    });
}

/// Age of the fade group covering `index`, if any. Groups are bounded by
/// `TEXT_FADE_WEIGHTS.len()` (at most two), so this is constant-time.
fn fade_age_at(groups: &[FadeGroup], index: usize) -> Option<usize> {
    groups
        .iter()
        .find(|group| group.start <= index && index < group.end)
        .map(|group| group.age)
}

/// Character-paced transcript lane with a separately admitted stable prefix.
#[derive(Debug, Clone, Default)]
pub struct RevealTrack {
    signature: RevealSignature,
    initialized: bool,
    revealed: usize,
    admitted: usize,
    stable_frontier: usize,
    finite: bool,
    rate: u16,
    groups: Vec<FadeGroup>,
    reveal_due: Option<Instant>,
    fade_due: Option<Instant>,
    hold_started: Option<Instant>,
    last_growth: Option<Instant>,
    admission_due: Option<Instant>,
}

impl RevealTrack {
    /// Reconcile a complete finite target. Retained for finite fixtures and
    /// callers that do not need streaming admission.
    pub fn reconcile(
        &mut self,
        signature: RevealSignature,
        finite: bool,
        now: Instant,
        chars_per_second: u16,
    ) -> bool {
        let admitted = signature.grapheme_count();
        self.reconcile_admitted(signature, admitted, finite, now, chars_per_second)
    }

    /// Reconcile complete semantic text while allowing only `stable_frontier`
    /// graphemes into the visible character queue. Settlement always admits
    /// the complete target; held streaming tails are bounded by two deadlines.
    pub fn reconcile_admitted(
        &mut self,
        signature: RevealSignature,
        stable_frontier: usize,
        finite: bool,
        now: Instant,
        chars_per_second: u16,
    ) -> bool {
        let rate = chars_per_second.min(crate::config::RevealRate::MAX);
        let target_len = signature.grapheme_count();
        let stable_frontier = stable_frontier.min(target_len);
        let mut changed = false;
        let old_len = self.signature.grapheme_count();
        let signature_changed = !self.initialized || self.signature != signature;
        let old_stable = self.stable_frontier;
        let previously_fully_admitted = self.admitted >= old_len;

        if !self.initialized {
            self.initialized = true;
            self.signature = signature;
            self.admitted = 0;
        } else if signature_changed {
            let common = common_prefix_graphemes(&self.signature, &signature);
            if common < self.revealed {
                self.revealed = common;
                changed = true;
            }
            self.admitted = self.admitted.min(common);
            truncate_fade_groups(&mut self.groups, self.revealed);
            self.signature = signature;
        }

        self.finite = finite;
        self.stable_frontier = stable_frontier;
        if finite {
            self.admitted = target_len;
            self.clear_hold();
        } else {
            self.admitted = self.admitted.max(stable_frontier).min(target_len);
            if self.admitted < target_len {
                let starts_new_hold = self.hold_started.is_none()
                    || stable_frontier > old_stable
                    || (signature_changed && previously_fully_admitted);
                if starts_new_hold {
                    self.hold_started = Some(now);
                }
                if signature_changed {
                    self.last_growth = Some(now);
                }
                let idle = self.last_growth.unwrap_or(now) + STREAM_IDLE_HOLD;
                let maximum = self.hold_started.unwrap_or(now) + STREAM_MAX_HOLD;
                self.admission_due = Some(idle.min(maximum));
            } else {
                self.clear_hold();
            }
        }

        self.update_rate(rate, now);
        if rate == 0 {
            changed |= self.revealed != self.admitted || !self.groups.is_empty();
            self.revealed = self.admitted;
            self.groups.clear();
            self.reveal_due = None;
            self.fade_due = None;
            return changed;
        }

        if self.reveal_due.is_none() && self.revealed < self.admitted {
            self.reveal_next(1, now);
            changed = true;
        }
        self.schedule_reveal(now);
        changed
    }

    /// Advance at most one due admission, fade, and character-batch step.
    pub fn tick(&mut self, now: Instant, chars_per_second: u16) -> bool {
        let rate = chars_per_second.min(crate::config::RevealRate::MAX);
        self.update_rate(rate, now);
        if rate == 0 {
            let changed = self.revealed != self.admitted || !self.groups.is_empty();
            self.revealed = self.admitted;
            self.groups.clear();
            self.reveal_due = None;
            self.fade_due = None;
            return changed;
        }

        let mut changed = false;
        if self.fade_due.is_some_and(|due| due <= now) {
            self.fade_due = None;
            changed |= age_fade_groups(&mut self.groups);
            if !self.groups.is_empty() {
                self.fade_due = Some(now + MIN_REVEAL_FRAME_INTERVAL);
            }
        }

        let mut admitted_now = false;
        if self.admission_due.is_some_and(|due| due <= now) {
            self.admitted = self.signature.grapheme_count();
            self.clear_hold();
            admitted_now = true;
        }

        let reveal_was_due = self.reveal_due.is_some_and(|due| due <= now);
        if reveal_was_due {
            self.reveal_due = None;
            if self.revealed < self.admitted {
                self.reveal_next(reveal_batch_size(rate), now);
                changed = true;
            }
        } else if admitted_now && self.reveal_due.is_none() && self.revealed < self.admitted {
            self.reveal_next(1, now);
            changed = true;
        }
        self.schedule_reveal(now);
        changed
    }

    pub fn next_due(&self) -> Option<Instant> {
        earliest_deadline(
            earliest_deadline(self.admission_due, self.reveal_due),
            self.fade_due,
        )
    }

    pub fn revealed(&self) -> usize {
        self.revealed
    }

    pub fn admitted(&self) -> usize {
        self.admitted
    }

    pub fn is_complete(&self) -> bool {
        self.initialized
            && self.finite
            && self.admitted >= self.signature.grapheme_count()
            && self.revealed >= self.signature.grapheme_count()
            && self.groups.is_empty()
            && self.admission_due.is_none()
    }

    pub fn has_pending_work(&self) -> bool {
        self.next_due().is_some()
    }

    fn update_rate(&mut self, rate: u16, now: Instant) {
        if self.rate == rate {
            return;
        }
        self.rate = rate;
        if self.reveal_due.is_some() {
            self.reveal_due = Some(now + reveal_interval(rate));
        }
    }

    fn schedule_reveal(&mut self, now: Instant) {
        if self.rate > 0 && self.revealed < self.admitted && self.reveal_due.is_none() {
            self.reveal_due = Some(now + reveal_interval(self.rate));
        } else if self.revealed >= self.admitted {
            self.reveal_due = None;
        }
    }

    fn reveal_next(&mut self, requested: usize, now: Instant) {
        let start = self.revealed;
        let count = requested.min(self.admitted.saturating_sub(start));
        if count == 0 {
            return;
        }
        self.revealed += count;
        self.groups.push(FadeGroup {
            start,
            end: self.revealed,
            age: 0,
        });
        if self.fade_due.is_none() {
            self.fade_due = Some(now + MIN_REVEAL_FRAME_INTERVAL);
        }
    }

    fn clear_hold(&mut self) {
        self.hold_started = None;
        self.last_growth = None;
        self.admission_due = None;
    }

    fn fade_age_at(&self, index: usize) -> Option<usize> {
        fade_age_at(&self.groups, index)
    }
}

/// Wrapped-display-row paced Preview lane. Progress is stored as a semantic
/// grapheme frontier, while row boundaries are refreshed for the current width.
#[derive(Debug, Clone, Default)]
pub struct LineRevealTrack {
    signature: RevealSignature,
    row_ends: Vec<usize>,
    initialized: bool,
    revealed: usize,
    rate: u16,
    groups: Vec<FadeGroup>,
    reveal_due: Option<Instant>,
    fade_due: Option<Instant>,
}

impl LineRevealTrack {
    pub fn reconcile(
        &mut self,
        wrapped_lines: &[Line<'static>],
        now: Instant,
        lines_per_second: u16,
    ) -> bool {
        let signature = RevealSignature::from_lines(wrapped_lines);
        let target_len = signature.grapheme_count();
        let row_ends = row_ends(wrapped_lines);
        let rate = lines_per_second.min(crate::config::RevealRate::MAX);
        let mut changed = false;

        if !self.initialized {
            self.initialized = true;
            self.signature = signature;
        } else if self.signature != signature {
            let common = common_prefix_graphemes(&self.signature, &signature);
            if common < self.revealed {
                self.revealed = common;
                changed = true;
            }
            truncate_fade_groups(&mut self.groups, self.revealed);
            self.signature = signature;
        }
        self.row_ends = row_ends;
        self.update_rate(rate, now);

        if rate == 0 {
            changed |= self.revealed != target_len || !self.groups.is_empty();
            self.revealed = target_len;
            self.groups.clear();
            self.reveal_due = None;
            self.fade_due = None;
            return changed;
        }

        if self.reveal_due.is_none() && self.next_row_end().is_some() {
            self.reveal_next_rows(1, now);
            changed = true;
        }
        self.schedule_reveal(now);
        changed
    }

    pub fn tick(&mut self, now: Instant, lines_per_second: u16) -> bool {
        let rate = lines_per_second.min(crate::config::RevealRate::MAX);
        self.update_rate(rate, now);
        if rate == 0 {
            let target = self.signature.grapheme_count();
            let changed = self.revealed != target || !self.groups.is_empty();
            self.revealed = target;
            self.groups.clear();
            self.reveal_due = None;
            self.fade_due = None;
            return changed;
        }

        let mut changed = false;
        if self.fade_due.is_some_and(|due| due <= now) {
            self.fade_due = None;
            changed |= age_fade_groups(&mut self.groups);
            if !self.groups.is_empty() {
                self.fade_due = Some(now + MIN_REVEAL_FRAME_INTERVAL);
            }
        }
        if self.reveal_due.is_some_and(|due| due <= now) {
            self.reveal_due = None;
            if self.next_row_end().is_some() {
                self.reveal_next_rows(reveal_batch_size(rate), now);
                changed = true;
            }
        }
        self.schedule_reveal(now);
        changed
    }

    pub fn next_due(&self) -> Option<Instant> {
        earliest_deadline(self.reveal_due, self.fade_due)
    }

    pub fn revealed(&self) -> usize {
        self.revealed
    }

    pub fn is_complete(&self) -> bool {
        self.initialized
            && self.revealed >= self.signature.grapheme_count()
            && self.groups.is_empty()
    }

    fn update_rate(&mut self, rate: u16, now: Instant) {
        if self.rate == rate {
            return;
        }
        self.rate = rate;
        if self.reveal_due.is_some() {
            self.reveal_due = Some(now + reveal_interval(rate));
        }
    }

    fn next_row_end(&self) -> Option<usize> {
        self.row_ends
            .iter()
            .copied()
            .find(|end| *end > self.revealed)
    }

    fn reveal_next_rows(&mut self, requested: usize, now: Instant) {
        let mut target = self.revealed;
        for _ in 0..requested {
            let Some(end) = self.row_ends.iter().copied().find(|end| *end > target) else {
                break;
            };
            target = end;
        }
        if target == self.revealed {
            return;
        }
        let start = self.revealed;
        self.revealed = target;
        self.groups.push(FadeGroup {
            start,
            end: target,
            age: 0,
        });
        if self.fade_due.is_none() {
            self.fade_due = Some(now + MIN_REVEAL_FRAME_INTERVAL);
        }
    }

    fn schedule_reveal(&mut self, now: Instant) {
        if self.rate > 0 && self.next_row_end().is_some() && self.reveal_due.is_none() {
            self.reveal_due = Some(now + reveal_interval(self.rate));
        } else if self.next_row_end().is_none() {
            self.reveal_due = None;
        }
    }

    fn fade_age_at(&self, index: usize) -> Option<usize> {
        fade_age_at(&self.groups, index)
    }
}

fn row_ends(lines: &[Line<'static>]) -> Vec<usize> {
    let mut total = 0usize;
    let mut ends = Vec::new();
    for line in lines {
        let text = line_text(line);
        total += text.graphemes(true).count();
        if !text.trim().is_empty() && ends.last().copied() != Some(total) {
            ends.push(total);
        }
    }
    if ends.is_empty() && total > 0 {
        ends.push(total);
    }
    ends
}

/// Number of visible units in one bounded animation-frame batch.
pub fn reveal_batch_size(units_per_second: u16) -> usize {
    let rate = units_per_second.min(crate::config::RevealRate::MAX);
    if rate == 0 {
        return 0;
    }
    let interval_millis = MIN_REVEAL_FRAME_INTERVAL.as_millis() as u64;
    usize::try_from((u64::from(rate) * interval_millis).div_ceil(1000))
        .expect("reveal batch size fits usize")
        .max(1)
}

pub fn reveal_interval(units_per_second: u16) -> Duration {
    match units_per_second.min(crate::config::RevealRate::MAX) {
        0 => Duration::ZERO,
        rate => Duration::from_secs_f64(reveal_batch_size(rate) as f64 / f64::from(rate)),
    }
}

pub fn apply_reveal(
    lines: Vec<Line<'static>>,
    track: &RevealTrack,
    background: Color,
    fallback_foreground: Color,
    fade_enabled: bool,
) -> Vec<Line<'static>> {
    apply_reveal_with_ages(
        lines,
        track.revealed,
        |index| track.fade_age_at(index),
        background,
        fallback_foreground,
        fade_enabled,
        TEXT_FADE_WEIGHTS,
    )
}

pub fn apply_line_reveal(
    lines: Vec<Line<'static>>,
    track: &LineRevealTrack,
    background: Color,
    fallback_foreground: Color,
    fade_enabled: bool,
) -> Vec<Line<'static>> {
    apply_reveal_with_ages(
        lines,
        track.revealed,
        |index| track.fade_age_at(index),
        background,
        fallback_foreground,
        fade_enabled,
        TEXT_FADE_WEIGHTS,
    )
}

#[cfg(test)]
fn apply_reveal_with_profile(
    lines: Vec<Line<'static>>,
    revealed: usize,
    drain: usize,
    background: Color,
    fallback_foreground: Color,
    fade_enabled: bool,
    profile: &[f64],
) -> Vec<Line<'static>> {
    let ages = (0..revealed)
        .map(|index| revealed.saturating_sub(1).saturating_sub(index) + drain)
        .collect::<Vec<_>>();
    apply_reveal_with_ages(
        lines,
        revealed,
        |index| ages.get(index).copied(),
        background,
        fallback_foreground,
        fade_enabled,
        profile,
    )
}

fn apply_reveal_with_ages<F>(
    lines: Vec<Line<'static>>,
    revealed: usize,
    fade_age: F,
    background: Color,
    fallback_foreground: Color,
    fade_enabled: bool,
    profile: &[f64],
) -> Vec<Line<'static>>
where
    F: Fn(usize) -> Option<usize>,
{
    if revealed == 0 {
        return Vec::new();
    }
    let mut remaining = revealed;
    let mut global_index = 0usize;
    let mut output = Vec::new();

    for line in lines {
        if remaining == 0 {
            break;
        }
        let styled = styled_graphemes(&line, fallback_foreground);
        if styled.is_empty() {
            output.push(line);
            continue;
        }
        let take = remaining.min(styled.len());
        let mut spans = Vec::new();
        for grapheme in styled.into_iter().take(take) {
            let mut style = grapheme.style;
            if fade_enabled {
                if let Some(weight) = fade_age(global_index).and_then(|age| profile.get(age)) {
                    let foreground = style.fg.unwrap_or(fallback_foreground);
                    style = style.fg(blend_rgb(background, foreground, *weight));
                }
            }
            push_merged_span(&mut spans, grapheme.text, style);
            global_index += 1;
        }
        remaining -= take;
        let mut visible = line;
        visible.spans = spans;
        output.push(visible);
    }
    output
}

pub fn blend_rgb(background: Color, foreground: Color, weight: f64) -> Color {
    let weight = weight.clamp(0.0, 1.0);
    match (background, foreground) {
        (Color::Rgb(br, bg, bb), Color::Rgb(fr, fg, fb)) => {
            let channel = |background: u8, foreground: u8| {
                (f64::from(background) + (f64::from(foreground) - f64::from(background)) * weight)
                    .round() as u8
            };
            Color::Rgb(channel(br, fr), channel(bg, fg), channel(bb, fb))
        }
        _ => foreground,
    }
}

#[derive(Debug)]
struct StyledGrapheme {
    text: String,
    style: Style,
}

fn styled_graphemes(line: &Line<'static>, fallback_foreground: Color) -> Vec<StyledGrapheme> {
    let mut text = String::new();
    let mut runs = Vec::new();
    for span in &line.spans {
        let start = text.len();
        text.push_str(span.content.as_ref());
        let end = text.len();
        if start != end {
            let mut style = line.style.patch(span.style);
            if style.fg.is_none() {
                style = style.fg(fallback_foreground);
            }
            runs.push((start, end, style));
        }
    }
    text.grapheme_indices(true)
        .map(|(start, grapheme)| {
            let style = runs
                .iter()
                .find(|(run_start, run_end, _)| *run_start <= start && start < *run_end)
                .map(|(_, _, style)| *style)
                .unwrap_or_else(|| line.style.fg(fallback_foreground));
            StyledGrapheme {
                text: grapheme.to_owned(),
                style,
            }
        })
        .collect()
}

fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn push_merged_span(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if let Some(last) = spans.last_mut() {
        if last.style == style {
            last.content.to_mut().push_str(&text);
            return;
        }
    }
    spans.push(Span::styled(text, style));
}

fn common_prefix_graphemes(old: &RevealSignature, new: &RevealSignature) -> usize {
    let limit = old.grapheme_starts.len().min(new.grapheme_starts.len());
    let mut common = 0;
    while common < limit && old.grapheme(common) == new.grapheme(common) {
        common += 1;
    }
    common
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    fn text(lines: &[Line<'static>]) -> String {
        lines.iter().map(line_text).collect::<Vec<_>>().join("|")
    }

    #[test]
    fn profile_preserves_styles_and_fades_only_active_group() {
        let start = Instant::now();
        let lines = vec![Line::styled(
            "abc",
            Style::default()
                .fg(Color::Rgb(100, 200, 50))
                .bg(Color::Rgb(7, 8, 9))
                .add_modifier(Modifier::BOLD),
        )];
        let mut track = RevealTrack::default();
        track.reconcile(RevealSignature::from_lines(&lines), true, start, 120);
        let output = apply_reveal(lines, &track, Color::Rgb(0, 0, 0), Color::White, true);
        assert_eq!(text(&output), "a");
        assert_eq!(output[0].spans[0].style.fg, Some(Color::Rgb(22, 43, 11)));
        assert_eq!(output[0].spans[0].style.bg, Some(Color::Rgb(7, 8, 9)));
        assert!(output[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn variable_profile_support_remains_generic() {
        let output = apply_reveal_with_profile(
            vec![Line::styled("abcd", Color::Rgb(100, 100, 100))],
            4,
            0,
            Color::Rgb(0, 0, 0),
            Color::White,
            true,
            &[0.1, 0.2, 0.3],
        );
        let colors = output[0]
            .spans
            .iter()
            .map(|span| span.style.fg)
            .collect::<Vec<_>>();
        assert_eq!(colors[0], Some(Color::Rgb(100, 100, 100)));
        assert_eq!(colors[1], Some(Color::Rgb(30, 30, 30)));
        assert_eq!(colors[2], Some(Color::Rgb(20, 20, 20)));
        assert_eq!(colors[3], Some(Color::Rgb(10, 10, 10)));
    }

    #[test]
    fn graphemes_are_not_split() {
        let lines = vec![Line::from("e\u{301} 👩\u{200d}💻 x")];
        assert_eq!(RevealSignature::from_lines(&lines).grapheme_count(), 5);
    }

    #[test]
    fn streaming_fade_continues_after_visible_queue_empties() {
        let start = Instant::now();
        let lines = vec![Line::styled("a", Color::Rgb(100, 100, 100))];
        let mut track = RevealTrack::default();
        track.reconcile(RevealSignature::from_lines(&lines), false, start, 16);
        assert_eq!(track.revealed(), 1);
        let fade_one = start + MIN_REVEAL_FRAME_INTERVAL;
        assert_eq!(track.next_due(), Some(fade_one));
        assert!(track.tick(fade_one, 16));
        let medium = apply_reveal(
            lines.clone(),
            &track,
            Color::Rgb(0, 0, 0),
            Color::White,
            true,
        );
        assert_eq!(medium[0].spans[0].style.fg, Some(Color::Rgb(53, 53, 53)));
        assert!(track.tick(fade_one + MIN_REVEAL_FRAME_INTERVAL, 16));
        assert_eq!(track.next_due(), None);
        let restored = apply_reveal(lines, &track, Color::Rgb(0, 0, 0), Color::White, true);
        assert_eq!(
            restored[0].spans[0].style.fg,
            Some(Color::Rgb(100, 100, 100))
        );
        assert!(!track.is_complete(), "open stream stays reusable");
    }

    #[test]
    fn later_append_restarts_idle_stream_without_recoloring_prefix() {
        let start = Instant::now();
        let mut track = RevealTrack::default();
        track.reconcile(
            RevealSignature::from_lines(&[Line::from("a")]),
            false,
            start,
            16,
        );
        track.tick(start + MIN_REVEAL_FRAME_INTERVAL, 16);
        track.tick(start + MIN_REVEAL_FRAME_INTERVAL * 2, 16);
        assert!(track.reconcile(
            RevealSignature::from_lines(&[Line::from("ab")]),
            false,
            start + Duration::from_secs(1),
            16,
        ));
        assert_eq!(track.revealed(), 2);
        assert_eq!(track.fade_age_at(0), None);
        assert_eq!(track.fade_age_at(1), Some(0));
    }

    #[test]
    fn admission_waits_then_flushes_on_idle_or_settlement() {
        let start = Instant::now();
        let signature = RevealSignature::from_lines(&[Line::from("hello")]);
        let mut track = RevealTrack::default();
        assert!(!track.reconcile_admitted(signature.clone(), 0, false, start, 16));
        assert_eq!(track.revealed(), 0);
        assert_eq!(track.next_due(), Some(start + STREAM_IDLE_HOLD));
        assert!(track.tick(start + STREAM_IDLE_HOLD, 16));
        assert_eq!(track.revealed(), 1);

        let mut settled = RevealTrack::default();
        settled.reconcile_admitted(signature.clone(), 0, false, start, 16);
        assert!(settled.reconcile_admitted(
            signature,
            0,
            true,
            start + Duration::from_millis(20),
            16,
        ));
        assert_eq!(settled.revealed(), 1);
    }

    #[test]
    fn reveal_and_fade_due_together_age_old_group_before_new_group() {
        let start = Instant::now();
        let mut track = RevealTrack::default();
        track.reconcile(
            RevealSignature::from_lines(&[Line::from("ab")]),
            true,
            start,
            120,
        );
        let due = start + reveal_interval(120);
        assert!(track.tick(due, 120));
        assert_eq!(track.revealed(), 2);
        assert_eq!(track.fade_age_at(0), Some(1));
        assert_eq!(track.fade_age_at(1), Some(0));
    }

    #[test]
    fn delayed_tick_does_not_catch_up_batches() {
        let start = Instant::now();
        let mut track = RevealTrack::default();
        track.reconcile(
            RevealSignature::from_lines(&[Line::from("abcd")]),
            true,
            start,
            16,
        );
        assert!(track.tick(start + Duration::from_secs(1), 16));
        assert_eq!(track.revealed(), 2);
    }

    #[test]
    fn zero_rate_reveals_the_admitted_target_immediately() {
        let start = Instant::now();
        let mut track = RevealTrack::default();
        track.reconcile(
            RevealSignature::from_lines(&[Line::from("abc")]),
            true,
            start,
            0,
        );
        assert_eq!(track.revealed(), 3);
        assert!(track.is_complete());
        assert_eq!(track.next_due(), None);
    }

    #[test]
    fn width_only_line_reflow_keeps_signature() {
        assert_eq!(
            RevealSignature::from_lines(&[Line::from("ab"), Line::from("cd")]),
            RevealSignature::from_lines(&[Line::from("a"), Line::from("bcd")])
        );
    }

    #[test]
    fn preview_track_reveals_wrapped_rows_not_graphemes() {
        let start = Instant::now();
        let lines = vec![Line::from("abc"), Line::from("def"), Line::from("ghi")];
        let mut track = LineRevealTrack::default();
        assert!(track.reconcile(&lines, start, 30));
        assert_eq!(track.revealed(), 3);
        let due = start + reveal_interval(30);
        assert!(track.tick(due, 30));
        assert_eq!(track.revealed(), 6);
        assert_eq!(
            text(&apply_line_reveal(
                lines,
                &track,
                Color::Black,
                Color::White,
                false
            )),
            "abc|def"
        );
    }

    #[test]
    fn preview_resize_preserves_semantic_frontier() {
        let start = Instant::now();
        let mut track = LineRevealTrack::default();
        track.reconcile(&[Line::from("abcd"), Line::from("efgh")], start, 30);
        assert_eq!(track.revealed(), 4);
        track.reconcile(
            &[
                Line::from("ab"),
                Line::from("cd"),
                Line::from("ef"),
                Line::from("gh"),
            ],
            start + Duration::from_millis(1),
            30,
        );
        assert_eq!(track.revealed(), 4);
    }
}
