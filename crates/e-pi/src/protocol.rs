//! Typed subset of Pi's documented 0.85.x RPC protocol.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcCommand {
    Prompt {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: String,
        #[serde(rename = "streamingBehavior", skip_serializing_if = "Option::is_none")]
        streaming_behavior: Option<StreamingBehavior>,
    },
    ClearQueue {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    Abort {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    NewSession {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    GetState {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    GetSessionStats {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    GetMessages {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    GetCommands {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    GetAvailableModels {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    SetModel {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
    },
    SetThinkingLevel {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        level: String,
    },
    Compact {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "customInstructions", skip_serializing_if = "Option::is_none")]
        custom_instructions: Option<String>,
    },
    SwitchSession {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "sessionPath")]
        session_path: String,
    },
    ExtensionUiResponse {
        id: String,
        #[serde(flatten)]
        response: ExtensionUiResponse,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum StreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ExtensionUiResponse {
    Value { value: String },
    Confirmed { confirmed: bool },
    Cancelled { cancelled: bool },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RpcRecord {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

impl RpcRecord {
    pub fn from_value(value: Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(value)
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        self.field(name).and_then(Value::as_str)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.field(name).and_then(Value::as_bool)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    #[serde(default)]
    pub id: Option<String>,
    pub command: String,
    pub success: bool,
    #[serde(default)]
    pub data: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionUiRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub prefill: Option<String>,
    #[serde(default)]
    pub notify_type: Option<String>,
    #[serde(default)]
    pub status_key: Option<String>,
    #[serde(default)]
    pub status_text: Option<String>,
    #[serde(default)]
    pub widget_key: Option<String>,
    #[serde(default)]
    pub widget_lines: Option<Vec<String>>,
    #[serde(default)]
    pub widget_placement: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

pub fn response(record: &RpcRecord) -> Result<RpcResponse, serde_json::Error> {
    let value = Value::Object(
        std::iter::once(("type".to_owned(), Value::String(record.kind.clone())))
            .chain(record.fields.clone())
            .collect(),
    );
    serde_json::from_value(value)
}

pub fn extension_ui_request(record: &RpcRecord) -> Result<ExtensionUiRequest, serde_json::Error> {
    let value = Value::Object(record.fields.clone().into_iter().collect());
    serde_json::from_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_use_pi_field_names() {
        let prompt = serde_json::to_value(RpcCommand::Prompt {
            id: Some("1".into()),
            message: "hello".into(),
            streaming_behavior: Some(StreamingBehavior::Steer),
        })
        .unwrap();
        assert_eq!(prompt["type"], "prompt");
        assert_eq!(prompt["streamingBehavior"], "steer");

        let model = serde_json::to_value(RpcCommand::SetModel {
            id: None,
            provider: "openai".into(),
            model_id: "gpt-5".into(),
        })
        .unwrap();
        assert_eq!(model["modelId"], "gpt-5");
    }

    #[test]
    fn parses_response_and_extension_request() {
        let record = RpcRecord::from_value(serde_json::json!({
            "type": "response", "id": "x", "command": "get_state",
            "success": true, "data": {"sessionId": "s"}
        }))
        .unwrap();
        assert_eq!(response(&record).unwrap().command, "get_state");

        let request = RpcRecord::from_value(serde_json::json!({
            "type": "extension_ui_request", "id": "u", "method": "select",
            "title": "Pick", "options": ["A", "B"]
        }))
        .unwrap();
        assert_eq!(extension_ui_request(&request).unwrap().options, ["A", "B"]);
    }
}
