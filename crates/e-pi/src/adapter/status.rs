//! Pi extension status projection; never replayed as native session history.

use e_tui::agent::{DelegatedStatus, DelegatedTask, Delegation, SessionEvent};
use serde::Deserialize;

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
    if key == super::btw::STATUS {
        return super::btw::status(adapter, text.as_deref().unwrap_or_default());
    }
    if key == "pie-subagents-v1" {
        return text
            .as_deref()
            .and_then(|json| delegation(json, adapter.native_session_id()))
            .map(|state| {
                AdapterOutput::event(e_tui::AgentEvent::Session(SessionEvent::Delegation(state)))
            })
            .unwrap_or_default();
    }
    if key == FAST_STATUS_KEY {
        adapter.fast_enabled = text.as_deref() == Some("fast");
        return AdapterOutput::event(e_tui::AgentEvent::Session(SessionEvent::FastMode(
            adapter.fast_enabled,
        )));
    }
    AdapterOutput::default()
}

fn delegation(json: &str, session: &str) -> Option<Delegation> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Snapshot {
        session_id: String,
        enabled: bool,
        entries: Vec<Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        slot: usize,
        label: String,
        status: String,
    }
    if json.len() > 2048 {
        return None;
    }
    let snap: Snapshot = serde_json::from_str(json).ok()?;
    if snap.session_id != session
        || snap.entries.len() > 6
        || (!snap.enabled && !snap.entries.is_empty())
    {
        return None;
    }
    let mut used = [false; 6];
    let entries = snap
        .entries
        .into_iter()
        .map(|entry| {
            let occupied = used.get_mut(entry.slot)?;
            if *occupied
                || entry.label.is_empty()
                || entry.label.chars().count() > 20
                || !entry.label.chars().all(char::is_alphanumeric)
            {
                return None;
            }
            *occupied = true;
            let status = match entry.status.as_str() {
                "running" => DelegatedStatus::Running,
                "done" => DelegatedStatus::Done,
                "failed" => DelegatedStatus::Failed,
                _ => return None,
            };
            Some(DelegatedTask {
                slot: entry.slot,
                label: entry.label,
                status,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Delegation {
        enabled: snap.enabled,
        entries,
    })
}
