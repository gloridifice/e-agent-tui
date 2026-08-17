//! Wire protocol between the dsh-tui client and the DSH bridge plugin.
//!
//! JSON messages share a `type` tag; field names are camelCase on the wire
//! (matching the bridge's JavaScript objects). Session events pass through
//! as opaque `serde_json::Value` for now; typed event views land with the
//! renderer (M2/M3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client → bridge messages.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ClientMessage {
    /// Authenticate and optionally attach to a specific live session.
    Hello {
        token: String,
        /// A fresh process with no resume target opens a NEW session on the
        /// bridge (each process shows one session).
        #[serde(skip_serializing_if = "Option::is_none")]
        resume_session_id: Option<String>,
        /// The directory the TUI was launched from. The bridge opens new
        /// sessions in this workspace (falling back to the attached
        /// session's header cwd when absent).
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        /// The configured default agent-preset mode for the session created
        /// at startup (the bridge falls back to `standard` when stale).
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
    },
    /// Ordinary user message (enters the agent inbox).
    Input { text: String },
    /// Slash command line.
    Command { line: String },
    /// Interrupt the current turn.
    Interrupt,
    /// Switch the connection to another live session.
    Attach { session_id: String },
    /// Request the session list (for the picker).
    ListSessions,
    /// Answer one pending approval.
    ApprovalAnswer { id: String, allow: bool },
    /// Submit answers for one pending user-question batch.
    AnswerQuestions { rpc_id: String, answers: Vec<QuestionAnswer> },
    /// Cancel one pending user-question batch (the host resolves the tool
    /// call as cancelled).
    CancelQuestions { rpc_id: String },
    /// Request older history: surface events with seq < `before_seq`,
    /// newest first from the stored log (lazy scroll-back paging).
    History { before_seq: u64, limit: usize },
    /// Read the login page state (providers / proxies / codex account).
    LoginGet,
    /// Store one provider's API key (empty clears it; the value itself is
    /// never read back — only its configured/source/hint view).
    LoginSetApiKey { provider: String, value: String },
    /// Begin the OpenAI Codex (ChatGPT) device-code login.
    LoginCodexStart,
    /// Cancel an in-flight Codex login.
    LoginCodexCancel,
    /// Create a custom proxy provider route.
    LoginProxyCreate { base_url: String, api_key: String, protocol: String, model: String },
    /// Remove one custom proxy provider route.
    LoginProxyDelete { id: String },
    /// Request the provider/model catalog (for the `/model` picker).
    ModelGet,
    /// Select the provider/model for the attached session.
    ModelSet { provider: String, model: String },
    /// Keepalive.
    Ping,
}

/// One question in an ask_user_question batch (as received from the host).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionItem {
    pub id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    #[serde(default)]
    pub multi_select: bool,
}

/// One selectable option of a question.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One answered question: the selected option labels (and an optional free
/// text answer for questions that offered no options).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuestionAnswer {
    pub id: String,
    pub selected: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

/// Bridge → client messages.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ServerMessage {
    Welcome {
        session_id: String,
        status: String,
        provider: Option<String>,
        model: Option<String>,
        /// Latest session/title of the attached session (absent until it
        /// has one; live updates ride ordinary `event` frames).
        #[serde(default)]
        title: Option<String>,
        /// Workspace path of the attached session (its header cwd), rendered
        /// right-aligned in the title row below the status bar.
        #[serde(default)]
        cwd: Option<String>,
    },
    Snapshot {
        events: Vec<Value>,
        /// True when the bridge capped the replay window.
        #[serde(default)]
        truncated: bool,
    },
    Event { event: Value },
    Status { status: String },
    /// One page of older history for the scroll-back request.
    History {
        events: Vec<Value>,
        /// False when the returned page reaches the oldest stored event.
        #[serde(default)]
        has_more: bool,
    },
    Sessions { sessions: Vec<SessionInfo> },
    /// The agent-preset roster the host offers; feeds the `/new <mode>`
    /// suggestion popup. Sent after every `welcome` (attach/`/new`/picker).
    Presets { presets: Vec<PresetInfo> },
    /// Title of the attached session, fetched from the projection store
    /// when the log is cold (resumed sessions) and the welcome frame could
    /// not carry one.
    Title { title: String },
    /// Login page state: the model providers (API-key entries), the saved
    /// proxy routes, and the codex account view. Secret values never cross
    /// the wire — only configured/source/hint views.
    Login {
        /// Providers that authenticate with an API key, in roster order.
        #[serde(default)]
        providers: Vec<ProviderInfo>,
        /// Custom proxy provider routes the user has added.
        #[serde(default)]
        proxies: Vec<ProxyInfo>,
        /// OpenAI Codex (ChatGPT subscription) account view.
        #[serde(default)]
        codex: Option<CodexInfo>,
        /// Message of the last rejected write (absent after a success).
        #[serde(default)]
        error: Option<String>,
    },
    /// Live OpenAI Codex device-login progress.
    LoginCodex {
        /// "pending" | "done" | "error".
        status: String,
        #[serde(default)]
        user_code: Option<String>,
        #[serde(default)]
        verification_uri: Option<String>,
        #[serde(default)]
        account_id: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },
    Approval {
        id: String,
        tool_name: String,
        reason: String,
        call_id: Option<String>,
    },
    /// One pending user-question batch to answer in the TUI.
    Question {
        rpc_id: String,
        session_id: String,
        questions: Vec<QuestionItem>,
    },
    /// A question batch settled (answered anywhere, or cancelled) — the TUI
    /// must drop its pending selection UI for that batch.
    QuestionResolved {
        question_rpc_id: String,
        outcome: String,
    },
    /// The provider/model catalog (for the `/model` picker) plus the current
    /// selection.
    Model {
        #[serde(default)]
        providers: Vec<ModelProviderInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current: Option<ModelCurrent>,
    },
    Error { code: String, message: String },
    Pong,
}

/// One session in the picker list.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub live: bool,
    pub created_at: u64,
}

/// One agent preset (a selectable `/new` mode) from the host roster.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u64>,
    /// Present when the preset's composition cannot mount — the client
    /// hides it from the mode popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken: Option<String>,
}

/// One API-key model provider on the login page.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub api_key_configured: bool,
    #[serde(default)]
    pub api_key_writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_hint: Option<String>,
}

/// One custom proxy provider route.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInfo {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub model: String,
}

/// OpenAI Codex (ChatGPT subscription) account view.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexInfo {
    pub logged_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

/// One model provider in the `/model` picker, with its model catalog.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}

/// One selectable model in the `/model` picker.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The current provider/model selection.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelCurrent {
    pub provider: String,
    pub model: String,
}

impl ClientMessage {
    pub fn to_wire(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl ServerMessage {
    pub fn from_wire(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_frame_parses_camel_case() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"question","rpcId":"r1","sessionId":"s1","questions":[{"id":"q1","question":"选哪个?","header":"Choose","options":[{"label":"A","description":"选项 A"}],"multiSelect":false}]}"#,
        )
        .expect("question parses");
        match msg {
            ServerMessage::Question { rpc_id, session_id, questions } => {
                assert_eq!(rpc_id, "r1");
                assert_eq!(session_id, "s1");
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].header.as_deref(), Some("Choose"));
                let options = questions[0].options.as_ref().unwrap();
                assert_eq!(options[0].label, "A");
                assert_eq!(options[0].description.as_deref(), Some("选项 A"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn question_resolved_parses() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"question-resolved","questionRpcId":"r1","outcome":"answered"}"#,
        )
        .expect("resolved parses");
        assert!(matches!(
            msg,
            ServerMessage::QuestionResolved { question_rpc_id, outcome }
                if question_rpc_id == "r1" && outcome == "answered"
        ));
    }

    #[test]
    fn answer_questions_serializes_for_the_bridge() {
        let msg = ClientMessage::AnswerQuestions {
            rpc_id: "r1".into(),
            answers: vec![
                QuestionAnswer { id: "q1".into(), selected: vec!["A".into()], custom: None },
                QuestionAnswer { id: "q2".into(), selected: vec![], custom: Some("自由".into()) },
            ],
        };
        let wire = msg.to_wire().unwrap();
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value["type"], "answer-questions");
        assert_eq!(value["rpcId"], "r1");
        assert_eq!(value["answers"][0]["selected"][0], "A");
        assert!(value["answers"][0].get("custom").is_none(), "absent custom is omitted");
        assert_eq!(value["answers"][1]["custom"], "自由");
        let cancel = ClientMessage::CancelQuestions { rpc_id: "r1".into() };
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&cancel.to_wire().unwrap()).unwrap()["type"],
            "cancel-questions"
        );
    }

    #[test]
    fn hello_carries_the_launch_cwd() {
        let msg = ClientMessage::Hello {
            token: "t".into(),
            resume_session_id: None,
            cwd: Some(r"D:\MyProjects\Chore\dsh".into()),
            mode: Some("standard".into()),
        };
        let value: serde_json::Value = serde_json::from_str(&msg.to_wire().unwrap()).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["cwd"], r"D:\MyProjects\Chore\dsh");
        assert_eq!(value["mode"], "standard", "the default mode rides hello");
        let bare = ClientMessage::Hello {
            token: "t".into(),
            resume_session_id: Some("s1".into()),
            cwd: None,
            mode: None,
        };
        let bare_value: serde_json::Value =
            serde_json::from_str(&bare.to_wire().unwrap()).unwrap();
        assert!(
            bare_value.get("cwd").is_none(),
            "absent cwd is omitted so the old bridge keeps its fallback"
        );
        assert!(
            bare_value.get("mode").is_none(),
            "mode is omitted when attaching (nothing to create)"
        );
    }

    #[test]
    fn welcome_parses_title_and_defaults_when_absent() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s1","status":"idle","provider":"p","model":"m","title":"标题行"}"#,
        )
        .expect("welcome with title parses");
        match msg {
            ServerMessage::Welcome { session_id, title, .. } => {
                assert_eq!(session_id, "s1");
                assert_eq!(title.as_deref(), Some("标题行"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // The old bridge sends no title — default to None, don't fail.
        let old = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#,
        )
        .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { title, .. } => assert_eq!(title, None),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn welcome_parses_cwd_and_defaults_when_absent() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s1","status":"idle","cwd":"D:\\MyProjects\\Chore\\dsh"}"#,
        )
        .expect("welcome with cwd parses");
        match msg {
            ServerMessage::Welcome { cwd, .. } => {
                assert_eq!(cwd.as_deref(), Some(r"D:\MyProjects\Chore\dsh"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // The old bridge sends no cwd — default to None, don't fail.
        let old = ServerMessage::from_wire(r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#)
            .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { cwd, .. } => assert_eq!(cwd, None),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn presets_frame_parses_camel_case() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"presets","presets":[{"id":"standard","name":"标准模式","order":1},{"id":"minimal","name":"极简模式","description":"双工具编码","order":3},{"id":"mine","broken":"missing composition"}]}"#,
        )
        .expect("presets parses");
        match msg {
            ServerMessage::Presets { presets } => {
                assert_eq!(presets.len(), 3);
                assert_eq!(presets[0].id, "standard");
                assert_eq!(presets[0].name.as_deref(), Some("标准模式"));
                assert_eq!(presets[0].order, Some(1));
                assert_eq!(presets[1].description.as_deref(), Some("双工具编码"));
                assert!(presets[2].broken.is_some());
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn title_frame_parses() {
        let msg = ServerMessage::from_wire(r#"{"type":"title","title":"冷会话标题"}"#)
            .expect("title parses");
        match msg {
            ServerMessage::Title { title } => assert_eq!(title, "冷会话标题"),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn login_frame_parses_providers_proxies_codex() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"login","providers":[{"id":"deepseek","name":"DeepSeek","apiKeyConfigured":true,"apiKeyWritable":true,"apiKeyHint":"…1234"}],"proxies":[{"id":"proxy-1","name":"我的代理","baseUrl":"https://example.com/v1","protocol":"openai-completions","model":"gpt-4o"}],"codex":{"loggedIn":false}}"#,
        )
        .expect("login parses");
        match msg {
            ServerMessage::Login { providers, proxies, codex, error } => {
                assert_eq!(providers.len(), 1);
                assert!(providers[0].api_key_configured);
                assert_eq!(providers[0].api_key_hint.as_deref(), Some("…1234"));
                assert_eq!(proxies.len(), 1);
                assert_eq!(proxies[0].protocol, "openai-completions");
                assert!(!codex.unwrap().logged_in);
                assert_eq!(error, None);
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // A rejected write rides the same frame with `error`.
        let failed = ServerMessage::from_wire(
            r#"{"type":"login","providers":[],"proxies":[],"error":"credentials-local: bad value"}"#,
        )
        .expect("login error parses");
        match failed {
            ServerMessage::Login { error, .. } => assert!(error.is_some()),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn login_up_frames_serialize() {
        let set = ClientMessage::LoginSetApiKey { provider: "deepseek".into(), value: "sk-test".into() };
        let v = serde_json::from_str::<serde_json::Value>(&set.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "login-set-api-key");
        assert_eq!(v["provider"], "deepseek");
        assert_eq!(v["value"], "sk-test");
        let create = ClientMessage::LoginProxyCreate {
            base_url: "https://x/v1".into(),
            api_key: "k".into(),
            protocol: "openai-completions".into(),
            model: "m".into(),
        };
        let v = serde_json::from_str::<serde_json::Value>(&create.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "login-proxy-create");
        assert_eq!(v["baseUrl"], "https://x/v1");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ClientMessage::LoginCodexStart.to_wire().unwrap()).unwrap()["type"],
            "login-codex-start"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ClientMessage::LoginGet.to_wire().unwrap()).unwrap()["type"],
            "login-get"
        );
    }

    #[test]
    fn login_codex_frame_parses() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"login-codex","status":"pending","userCode":"ABCD-EFGH","verificationUri":"https://auth.openai.com/codex/device"}"#,
        )
        .expect("login-codex parses");
        match msg {
            ServerMessage::LoginCodex { status, user_code, verification_uri, .. } => {
                assert_eq!(status, "pending");
                assert_eq!(user_code.as_deref(), Some("ABCD-EFGH"));
                assert_eq!(verification_uri.as_deref(), Some("https://auth.openai.com/codex/device"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn model_frame_parses_and_serializes() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"model","providers":[{"id":"deepseek","name":"DeepSeek","models":[{"id":"deepseek-v4-pro","name":"DeepSeek V4 Pro","description":"flagship"},{"id":"deepseek-v4","name":"DeepSeek V4"}]}],"current":{"provider":"deepseek","model":"deepseek-v4"}}"#,
        )
        .expect("model parses");
        match msg {
            ServerMessage::Model { providers, current } => {
                assert_eq!(providers.len(), 1);
                assert_eq!(providers[0].models.len(), 2);
                assert_eq!(providers[0].models[0].description.as_deref(), Some("flagship"));
                let cur = current.expect("current selection");
                assert_eq!(cur.provider, "deepseek");
                assert_eq!(cur.model, "deepseek-v4");
            }
            other => panic!("wrong variant: {other:?}"),
        }
        let set = ClientMessage::ModelSet {
            provider: "deepseek".into(),
            model: "deepseek-v4-pro".into(),
        };
        let v = serde_json::from_str::<serde_json::Value>(&set.to_wire().unwrap()).unwrap();
        assert_eq!(v["type"], "model-set");
        assert_eq!(v["provider"], "deepseek");
        assert_eq!(v["model"], "deepseek-v4-pro");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ClientMessage::ModelGet.to_wire().unwrap()).unwrap()["type"],
            "model-get"
        );
    }
}
