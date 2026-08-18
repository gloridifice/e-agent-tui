//! Wire protocol between the dsh-tui client and the DSH bridge plugin.

include!(concat!(env!("OUT_DIR"), "/wire_contract.rs"));

mod host_event;
pub use host_event::*;

mod messages;
pub use messages::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn dsh_event_fixture_covers_protocol_edges() {
        let events: Vec<Value> = serde_json::from_str(include_str!("../testdata/dsh-events.json"))
            .expect("bounded DSH fixture parses");
        assert!(events.len() <= 32, "fixture stays bounded");
        let types: std::collections::HashSet<&str> = events
            .iter()
            .filter_map(|event| event.get("type").and_then(Value::as_str))
            .collect();
        for required in [
            "user/message",
            "assistant/chunk",
            "assistant/message",
            "tool/result",
            "todo/write",
            "compaction/summary",
        ] {
            assert!(types.contains(required), "fixture contains {required}");
        }
        let replacement = events
            .iter()
            .find(|event| {
                event
                    .get("surfaceOp")
                    .and_then(|op| op.get("op"))
                    .and_then(Value::as_str)
                    == Some("replace")
            })
            .expect("fixture carries replace metadata");
        assert_eq!(replacement["time"], 1210);
        assert!(replacement["sourceEventSeqs"].is_array());

        let typed: Vec<HostEvent> = events.into_iter().map(HostEvent::from_value).collect();
        let reasoning = typed.iter().find(|event| matches!(event.kind, HostEventKind::AssistantChunk { ref reasoning, .. } if reasoning == "think")).expect("reasoning delta typed");
        assert_eq!(reasoning.time_ms, Some(1040));
        let context = typed
            .iter()
            .find_map(|event| match &event.kind {
                HostEventKind::UserMessage { source, .. }
                    if source.form.as_deref() == Some("instructions") =>
                {
                    Some(source)
                }
                _ => None,
            })
            .expect("context source typed");
        assert_eq!(context.producer.as_deref(), Some("instructions"));
        assert!(typed
            .iter()
            .any(|event| matches!(event.kind, HostEventKind::ToolResult { is_error: true, .. })));
        let replacement = typed
            .iter()
            .find(|event| matches!(event.surface_op, Some(HostSurfaceOp::Replace { .. })))
            .expect("replacement typed");
        assert_eq!(replacement.source_event_seqs, vec![2, 3, 7, 9]);
    }

    #[test]
    fn extended_history_fixture_enters_client_replay_roster() {
        let events: Vec<Value> = serde_json::from_str(include_str!(
            "../../bridge/test/fixtures/session-events.json"
        ))
        .unwrap();
        let typed: Vec<HostEvent> = events.into_iter().map(HostEvent::from_value).collect();
        for required in [
            "command/run",
            "compaction/summary",
            "llm/retry",
            "tool/code-dispatch",
            "tool-workflow/run-end",
            "todo/write",
        ] {
            let event = typed
                .iter()
                .find(|event| {
                    event.as_value().get("type").and_then(Value::as_str) == Some(required)
                })
                .unwrap();
            assert!(
                event.is_replay_relevant(),
                "{required} survives snapshot replay filtering"
            );
        }
        for audit in [
            "approval/asked",
            "request/header",
            "session/title-llm-request",
        ] {
            let event = typed
                .iter()
                .find(|event| event.as_value().get("type").and_then(Value::as_str) == Some(audit))
                .unwrap();
            assert!(!event.is_replay_relevant(), "{audit} remains audit-only");
        }
        assert!(typed.iter().any(|event| matches!(
            event.kind,
            HostEventKind::WorkflowAgentEnd {
                outcome: HostLifecycleOutcome::Success,
                ..
            }
        )));
        assert!(typed.iter().any(|event| matches!(
            event.kind,
            HostEventKind::WorkflowRunEnd {
                outcome: HostLifecycleOutcome::Success,
                ..
            }
        )));
    }

    #[test]
    fn host_events_are_typed_at_the_wire_boundary() {
        let message = ServerMessage::from_wire(
            r#"{"type":"event","event":{"seq":7,"type":"tool/call","data":{"callId":"c1","name":"read","arguments":"{\"file_path\":\"a.rs\"}","time":42}}}"#,
        )
        .expect("typed event parses");
        match message {
            ServerMessage::Event { event } => {
                assert_eq!(event.seq, Some(7));
                assert_eq!(event.time_ms, Some(42));
                assert_eq!(
                    event.kind,
                    HostEventKind::ToolCall {
                        call_id: "c1".into(),
                        name: "read".into(),
                        arguments: r#"{"file_path":"a.rs"}"#.into(),
                    }
                );
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn unknown_host_event_is_preserved_without_entering_the_model_schema() {
        let event = HostEvent::from_value(serde_json::json!({
            "seq": 9,
            "type": "future/event",
            "data": { "new": true }
        }));
        assert!(matches!(
            event.kind,
            HostEventKind::Unknown { event_type: Some(ref kind) } if kind == "future/event"
        ));
        assert_eq!(event.as_value()["data"]["new"], true);
    }

    #[test]
    fn question_frame_parses_camel_case() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"question","rpcId":"r1","sessionId":"s1","questions":[{"id":"q1","question":"选哪个?","header":"Choose","options":[{"label":"A","description":"选项 A"}],"multiSelect":false}]}"#,
        )
        .expect("question parses");
        match msg {
            ServerMessage::Question {
                rpc_id,
                session_id,
                questions,
            } => {
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
                QuestionAnswer {
                    id: "q1".into(),
                    selected: vec!["A".into()],
                    custom: None,
                },
                QuestionAnswer {
                    id: "q2".into(),
                    selected: vec![],
                    custom: Some("自由".into()),
                },
            ],
        };
        let wire = msg.to_wire().unwrap();
        let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
        assert_eq!(value["type"], "answer-questions");
        assert_eq!(value["rpcId"], "r1");
        assert_eq!(value["answers"][0]["selected"][0], "A");
        assert!(
            value["answers"][0].get("custom").is_none(),
            "absent custom is omitted"
        );
        assert_eq!(value["answers"][1]["custom"], "自由");
        let cancel = ClientMessage::CancelQuestions {
            rpc_id: "r1".into(),
        };
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
            protocol_version: WIRE_PROTOCOL_VERSION,
        };
        let value: serde_json::Value = serde_json::from_str(&msg.to_wire().unwrap()).unwrap();
        assert_eq!(value["type"], "hello");
        assert_eq!(value["cwd"], r"D:\MyProjects\Chore\dsh");
        assert_eq!(value["mode"], "standard", "the default mode rides hello");
        assert_eq!(value["protocolVersion"], WIRE_PROTOCOL_VERSION);
        let bare = ClientMessage::Hello {
            token: "t".into(),
            resume_session_id: Some("s1".into()),
            cwd: None,
            mode: None,
            protocol_version: WIRE_PROTOCOL_VERSION,
        };
        let bare_value: serde_json::Value = serde_json::from_str(&bare.to_wire().unwrap()).unwrap();
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
    fn welcome_parses_mode_title_and_defaults_when_absent() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"welcome","sessionId":"s1","status":"idle","provider":"p","model":"m","mode":"cordis","title":"标题行"}"#,
        )
        .expect("welcome with mode and title parses");
        match msg {
            ServerMessage::Welcome {
                session_id,
                mode,
                title,
                ..
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(mode.as_deref(), Some("cordis"));
                assert_eq!(title.as_deref(), Some("标题行"));
            }
            other => panic!("wrong variant: {other:?}"),
        }
        // An old bridge may send neither field — default to None, don't fail.
        let old =
            ServerMessage::from_wire(r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#)
                .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { mode, title, .. } => {
                assert_eq!(mode, None);
                assert_eq!(title, None);
            }
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
        let old =
            ServerMessage::from_wire(r#"{"type":"welcome","sessionId":"s2","status":"idle"}"#)
                .expect("old welcome parses");
        match old {
            ServerMessage::Welcome { cwd, .. } => assert_eq!(cwd, None),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn sessions_frame_marks_progressive_title_loading() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"sessions","titlesPending":true,"sessions":[{"id":"s1","title":"","live":false,"createdAt":1}]}"#,
        )
        .expect("sessions parses");
        assert!(matches!(
            msg,
            ServerMessage::Sessions {
                titles_pending: true,
                sessions,
            } if sessions.len() == 1 && sessions[0].id == "s1"
        ));
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
    fn skills_frame_parses() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"skills","skills":[{"name":"code-review","description":"Review a change"}]}"#,
        )
        .expect("skills parses");
        match msg {
            ServerMessage::Skills { skills } => {
                assert_eq!(skills.len(), 1);
                assert_eq!(skills[0].name, "code-review");
                assert_eq!(skills[0].description, "Review a change");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn command_directory_and_result_frames_parse() {
        let directory = ServerMessage::from_wire(
            r#"{"type":"commands","commands":[{"name":"feedback","description":"record feedback","input":{"hint":"<text>"}}]}"#,
        )
        .expect("command directory parses");
        match directory {
            ServerMessage::Commands { commands } => {
                assert_eq!(commands[0].name, "feedback");
                assert_eq!(commands[0].input.as_ref().unwrap().hint, "<text>");
            }
            other => panic!("wrong variant: {other:?}"),
        }

        let result = ServerMessage::from_wire(
            r#"{"type":"command-result","commandId":"cmd-1","kind":"success","text":"done"}"#,
        )
        .expect("command result parses");
        match result {
            ServerMessage::CommandResult {
                command_id,
                kind,
                text,
            } => {
                assert_eq!(command_id, "cmd-1");
                assert_eq!(kind, "success");
                assert_eq!(text.as_deref(), Some("done"));
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
    fn login_frame_parses_providers_proxies() {
        let msg = ServerMessage::from_wire(
            r#"{"type":"login","providers":[{"id":"deepseek","name":"DeepSeek","apiKeyConfigured":true,"apiKeyWritable":true,"apiKeyHint":"…1234"}],"proxies":[{"id":"proxy-1","name":"我的代理","baseUrl":"https://example.com/v1","protocol":"openai-completions","model":"gpt-4o"}]}"#,
        )
        .expect("login parses");
        match msg {
            ServerMessage::Login {
                providers,
                proxies,
                error,
            } => {
                assert_eq!(providers.len(), 1);
                assert!(providers[0].api_key_configured);
                assert_eq!(providers[0].api_key_hint.as_deref(), Some("…1234"));
                assert_eq!(proxies.len(), 1);
                assert_eq!(proxies[0].protocol, "openai-completions");
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
        let set = ClientMessage::LoginSetApiKey {
            provider: "deepseek".into(),
            value: "sk-test".into(),
        };
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
            serde_json::from_str::<serde_json::Value>(&ClientMessage::LoginGet.to_wire().unwrap())
                .unwrap()["type"],
            "login-get"
        );
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
                assert_eq!(
                    providers[0].models[0].description.as_deref(),
                    Some("flagship")
                );
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
            serde_json::from_str::<serde_json::Value>(&ClientMessage::ModelGet.to_wire().unwrap())
                .unwrap()["type"],
            "model-get"
        );
    }
}
