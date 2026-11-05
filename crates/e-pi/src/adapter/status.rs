//! Pi extension status projection; never replayed as native session history.

use e_tui::agent::{AgentEvent, SessionEvent};

use super::{AdapterOutput, PiAdapter, FAST_STATUS_KEY};

pub(super) fn update(
    adapter: &mut PiAdapter,
    key: Option<String>,
    text: Option<String>,
) -> AdapterOutput {
    let Some(key) = key.filter(|key| !key.is_empty()) else {
        return AdapterOutput::default();
    };
    let text = text.filter(|text| !text.trim().is_empty());
    if key == FAST_STATUS_KEY {
        adapter.fast_enabled = text.as_deref() == Some("fast");
        return AdapterOutput::event(e_tui::AgentEvent::Session(SessionEvent::FastMode(
            adapter.fast_enabled,
        )));
    }
    AdapterOutput::event(AgentEvent::Session(SessionEvent::ExtensionStatus {
        key,
        text,
    }))
}
