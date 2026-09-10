use std::time::{Duration, Instant};

use ratatui::style::Color;

use crate::{catalog::CatalogModel, color::lerp_rgb, theme::Theme};

const DURATION: Duration = Duration::from_millis(600);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Accent {
    Mist,
    Ember,
    Honey,
    Blush,
}

impl Accent {
    fn effort(id: Option<&str>) -> Self {
        match id {
            Some("max") => Self::Ember,
            Some("xhigh") => Self::Honey,
            Some("high") => Self::Blush,
            _ => Self::Mist,
        }
    }

    fn color(self, theme: &Theme) -> Color {
        match self {
            Self::Mist => theme.status_flash.model,
            Self::Ember => theme.status_flash.effort_max,
            Self::Honey => theme.status_flash.effort_xhigh,
            Self::Blush => theme.status_flash.effort_high,
        }
    }
}

struct Flash {
    accent: Accent,
    started: Instant,
    next_due: Instant,
    progress: f64,
}

impl Flash {
    fn new(accent: Accent, now: Instant) -> Self {
        Self {
            accent,
            started: now,
            next_due: now + FRAME_INTERVAL,
            progress: 0.0,
        }
    }

    fn color(&self, theme: &Theme, normal: Color) -> Color {
        let t = self.progress;
        lerp_rgb(self.accent.color(theme), normal, t * t * (3.0 - 2.0 * t))
    }
}

#[derive(PartialEq, Eq)]
struct Selection {
    route: (String, String),
    // Outer None is hidden; inner None is the visible provider default.
    effort: Option<Option<String>>,
}

#[derive(Default)]
pub(crate) struct StatusFlashes {
    observed: Option<Selection>,
    model: Option<Flash>,
    effort: Option<Flash>,
}

impl StatusFlashes {
    pub(crate) fn observe(&mut self, catalogs: &CatalogModel, now: Instant) {
        let Some(current) = &catalogs.current_model else {
            *self = Self::default();
            return;
        };
        let next = Selection {
            route: (current.provider.clone(), current.model.clone()),
            effort: catalogs.effort_status().map(|status| status.id),
        };
        if let Some(previous) = &self.observed {
            if previous.route != next.route {
                self.model = Some(Flash::new(Accent::Mist, now));
            }
            if previous.effort != next.effort {
                self.effort = next
                    .effort
                    .as_ref()
                    .map(|id| Flash::new(Accent::effort(id.as_deref()), now));
            }
        }
        self.observed = Some(next);
    }

    pub(crate) fn next_due(&self) -> Option<Instant> {
        [&self.model, &self.effort]
            .into_iter()
            .filter_map(|flash| flash.as_ref().map(|flash| flash.next_due))
            .min()
    }

    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for slot in [&mut self.model, &mut self.effort] {
            let Some(flash) = slot else { continue };
            if now < flash.next_due {
                continue;
            }
            changed = true;
            let elapsed = now.saturating_duration_since(flash.started);
            if elapsed >= DURATION {
                *slot = None;
            } else {
                flash.progress = elapsed.as_secs_f64() / DURATION.as_secs_f64();
                flash.next_due = now + FRAME_INTERVAL;
            }
        }
        changed
    }

    pub(crate) fn model_color(&self, theme: &Theme, normal: Color) -> Color {
        self.model
            .as_ref()
            .map_or(normal, |flash| flash.color(theme, normal))
    }

    pub(crate) fn effort_color(&self, theme: &Theme, normal: Color) -> Color {
        self.effort
            .as_ref()
            .map_or(normal, |flash| flash.color(theme, normal))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ModelDescriptor, ModelProvider, ModelReasoning, ModelSelection};

    fn catalog(provider: &str, effort: Option<&str>) -> CatalogModel {
        CatalogModel {
            current_model: Some(ModelSelection {
                provider: provider.into(),
                model: "model".into(),
                reasoning_effort: effort.map(str::to_owned),
            }),
            model_providers: vec![ModelProvider {
                id: provider.into(),
                name: provider.into(),
                models: vec![ModelDescriptor {
                    id: "model".into(),
                    name: "model".into(),
                    description: None,
                    context_window: None,
                    reasoning: Some(ModelReasoning {
                        efforts: vec![],
                        default_effort: None,
                    }),
                }],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn status_flash_timing_is_bounded_and_requests_a_final_frame() {
        let now = Instant::now();
        let mut flashes = StatusFlashes::default();
        flashes.observe(&catalog("a", Some("low")), now);
        assert_eq!(flashes.next_due(), None);
        flashes.observe(&catalog("b", Some("max")), now);
        assert_eq!(flashes.model.as_ref().unwrap().progress, 0.0);
        assert_eq!(flashes.next_due(), Some(now + FRAME_INTERVAL));
        assert!(!flashes.tick(now));
        assert!(flashes.tick(now + DURATION / 2));
        assert_eq!(flashes.model.as_ref().unwrap().progress, 0.5);
        assert_eq!(
            flashes.next_due(),
            Some(now + DURATION / 2 + FRAME_INTERVAL)
        );
        assert!(flashes.tick(now + DURATION));
        assert_eq!(flashes.next_due(), None);
        assert!(!flashes.tick(now + DURATION * 2));
    }

    #[test]
    fn status_flash_refresh_and_restart_are_independent() {
        let now = Instant::now();
        let later = now + Duration::from_millis(100);
        let mut flashes = StatusFlashes::default();
        flashes.observe(&catalog("a", Some("low")), now);
        flashes.observe(&catalog("b", Some("low")), now);
        assert!(flashes.effort.is_none());
        flashes.tick(later);
        flashes.observe(&catalog("b", Some("low")), later);
        assert_eq!(flashes.model.as_ref().unwrap().started, now);
        flashes.observe(&catalog("b", Some("high")), later);
        assert_eq!(flashes.model.as_ref().unwrap().started, now);
        assert_eq!(flashes.effort.as_ref().unwrap().started, later);
        assert_eq!(flashes.effort.as_ref().unwrap().accent, Accent::Blush);
        flashes.observe(&catalog("b", Some("max")), later + FRAME_INTERVAL);
        assert_eq!(flashes.effort.as_ref().unwrap().progress, 0.0);
        assert_eq!(flashes.effort.as_ref().unwrap().accent, Accent::Ember);
        flashes.observe(&catalog("c", Some("max")), later + FRAME_INTERVAL * 2);
        assert_eq!(
            flashes.model.as_ref().unwrap().started,
            later + FRAME_INTERVAL * 2
        );
        assert_eq!(
            flashes.effort.as_ref().unwrap().started,
            later + FRAME_INTERVAL
        );
    }

    #[test]
    fn status_flash_effort_uses_effective_ids_and_clears_hidden_entries() {
        let now = Instant::now();
        let mut flashes = StatusFlashes::default();
        flashes.observe(&catalog("a", Some("low")), now);
        for (id, accent) in [
            ("max", Accent::Ember),
            ("xhigh", Accent::Honey),
            ("high", Accent::Blush),
            ("medium", Accent::Mist),
            ("off", Accent::Mist),
        ] {
            let mut next = catalog("a", None);
            let reasoning = next.model_providers[0].models[0]
                .reasoning
                .as_mut()
                .unwrap();
            reasoning.default_effort = Some(id.into());
            reasoning.efforts.push(crate::agent::ReasoningEffort {
                id: id.into(),
                name: "Custom label".into(),
                description: None,
            });
            flashes.observe(&next, now);
            assert_eq!(flashes.effort.as_ref().unwrap().accent, accent);
            assert!(flashes.model.is_none());
        }
        flashes.observe(&catalog("a", None), now);
        assert_eq!(flashes.effort.as_ref().unwrap().accent, Accent::Mist);
        let mut hidden = catalog("a", Some("max"));
        hidden.model_providers[0].models[0].reasoning = None;
        flashes.observe(&hidden, now);
        assert!(flashes.effort.is_none());
        assert_eq!(flashes.next_due(), None);
        flashes.observe(&CatalogModel::default(), now);
        flashes.observe(&catalog("b", Some("high")), now);
        assert_eq!(flashes.next_due(), None);
    }
}
