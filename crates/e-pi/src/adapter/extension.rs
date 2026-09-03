//! Extension-UI request correlation and response projection for Pi.

use e_tui::{
    action::QuestionAnswer,
    agent::{
        timeline::TimelineFact, AgentEvent, InteractionEvent, Question, QuestionOption,
        SessionEvent, ToolCapability,
    },
};

use crate::protocol::{
    extension_ui_request, ExtensionUiRequest, ExtensionUiResponse, RpcCommand, RpcRecord,
};

use super::{AdapterOutput, PendingExtensionUi, PiAdapter};

pub(super) fn answer_question(
    adapter: &mut PiAdapter,
    request_id: String,
    answers: Vec<QuestionAnswer>,
) -> AdapterOutput {
    let Some(method) = adapter.extension_ui.remove(&request_id) else {
        return AdapterOutput::default();
    };
    if method == PendingExtensionUi::Confirm {
        return adapter.unsupported("invalid Pi confirmation response route");
    }
    let value = answers.into_iter().next().and_then(|answer| {
        answer
            .custom
            .filter(|value| !value.is_empty())
            .or_else(|| answer.selected.into_iter().next())
    });
    AdapterOutput::command(RpcCommand::ExtensionUiResponse {
        id: request_id,
        response: value.map_or(
            ExtensionUiResponse::Cancelled { cancelled: true },
            |value| ExtensionUiResponse::Value { value },
        ),
    })
}

pub(super) fn request(adapter: &mut PiAdapter, record: RpcRecord) -> AdapterOutput {
    let request = match extension_ui_request(&record) {
        Ok(request) => request,
        Err(error) => return adapter.protocol_error(error.to_string()),
    };
    match request.method.as_str() {
        "select" => question(adapter, request, PendingExtensionUi::Select, true),
        "input" => question(adapter, request, PendingExtensionUi::Input, false),
        "editor" => question(adapter, request, PendingExtensionUi::Editor, false),
        "confirm" => {
            adapter
                .extension_ui
                .insert(request.id.clone(), PendingExtensionUi::Confirm);
            AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Approval {
                id: request.id,
                capability: ToolCapability::Generic,
                label: request.title.unwrap_or_else(|| "Confirm".into()),
                reason: request.message.unwrap_or_default(),
            }))
        }
        "notify" => {
            let message = request.message.unwrap_or_default();
            if request.notify_type.as_deref() == Some("error") {
                AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Error {
                    code: "pi-notify".into(),
                    message,
                }))
            } else {
                adapter.timeline(TimelineFact::Custom {
                    namespace: "pi".into(),
                    kind: Some("notification".into()),
                    summary: Some(message),
                })
            }
        }
        "set_editor_text" => {
            AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::SetEditorText {
                text: request.text.unwrap_or_default(),
            }))
        }
        "setTitle" => AdapterOutput::event(AgentEvent::Session(SessionEvent::Title(
            request.title.unwrap_or_default(),
        ))),
        "setStatus" | "setWidget" => AdapterOutput::default(),
        _ => AdapterOutput::default(),
    }
}

pub(super) fn question(
    adapter: &mut PiAdapter,
    request: ExtensionUiRequest,
    method: PendingExtensionUi,
    has_options: bool,
) -> AdapterOutput {
    adapter.extension_ui.insert(request.id.clone(), method);
    let question = Question {
        id: "value".into(),
        question: request
            .message
            .or(request.placeholder)
            .or(request.prefill)
            .unwrap_or_else(|| request.title.clone().unwrap_or_default()),
        header: request.title,
        options: has_options.then(|| {
            request
                .options
                .into_iter()
                .map(|label| QuestionOption {
                    label,
                    description: None,
                })
                .collect()
        }),
        multi_select: false,
    };
    AdapterOutput::event(AgentEvent::Interaction(InteractionEvent::Question {
        request_id: request.id,
        session_id: adapter.session_id.clone(),
        questions: vec![question],
    }))
}
