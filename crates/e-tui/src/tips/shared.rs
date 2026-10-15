use super::{TipDefinition, TipPrerequisite as P, TipRequirement as R};
use crate::key_mapping::{Action as A, Scope as S};

pub(super) fn definitions() -> Vec<TipDefinition> {
    let prerequisite = |value| R::Prerequisite(value);
    let command = |name: &str| R::Command(name.into());
    let capability = |name: &str| R::Capability(name.into());
    vec![
        TipDefinition::bilingual("shared.reading-copy", "按 `Ctrl+R` 进入阅读模式，用 `y` 复制当前内容块。", "Press `Ctrl+R` for Reading mode, then `y` to copy the current block.", &[("Ctrl+R", S::Global, A::EnterReadMode), ("y", S::ReadMode, A::CopyBlock)], vec![]),
        TipDefinition::bilingual("shared.preview", "按 `Ctrl+P` 打开 `Preview`，查看完整内容。", "Press `Ctrl+P` to open `Preview` and view complete content.", &[("Ctrl+P", S::Global, A::TogglePreview)], vec![]),
        TipDefinition::bilingual("shared.copy-answer", "输入 `/copy`，复制最近一次已完成回复的 Markdown 原文。", "Use `/copy` to copy the latest completed answer's Markdown source.", &[], vec![command("copy"), prerequisite(P::CompletedAnswer)]),
        TipDefinition::bilingual("shared.send-asap", "智能体工作时按 `Enter` 可以发送补充消息，无需等待本轮会话结束。", "Press `Enter` while the agent works to send a message without waiting for the run to end.", &[("Enter", S::MessageWorking, A::SendAsap)], vec![prerequisite(P::AsapAdmission)]),
        TipDefinition::bilingual("shared.send-after-turn", "智能体工作时，按 `Ctrl+Enter` 发送消息，会在本轮会话结束后发送。", "While the agent works, `Ctrl+Enter` queues a message for after this run.", &[("Ctrl+Enter", S::MessageWorking, A::SendAfterTurn)], vec![]),
        TipDefinition::bilingual("shared.newline", "按 `Shift+Enter` 换行，按 `Enter` 发送。", "Press `Shift+Enter` for a new line and `Enter` to send.", &[("Shift+Enter", S::Message, A::NewLine), ("Enter", S::MessageWorking, A::SendAsap)], vec![prerequisite(P::AsapAdmission)]),
        TipDefinition::bilingual("shared.cancel-queue", "有排队消息时，`Esc` 优先撤回消息；队列空后才中断任务。", "With queued messages, `Esc` withdraws them before interrupting the task.", &[("Esc", S::Message, A::CancelOrInterrupt)], vec![prerequisite(P::PendingQueue), prerequisite(P::ShellAvailable)]),
        TipDefinition::bilingual("shared.history-views", "输入 `/history` 查看执行耗时，在历史页按 `Tab` 切换时间线。", "Use `/history` for execution times; press `Tab` there to switch to the timeline.", &[("Tab", S::History, A::ToggleView)], vec![command("history"), capability("history"), prerequisite(P::AttachedSession)]),
        TipDefinition::bilingual("shared.history-copy", "输入 `/history copy-10`，复制耗时最长的十项操作。", "Use `/history copy-10` to copy the ten slowest operations.", &[], vec![command("history"), capability("history"), prerequisite(P::AttachedSession)]),
        TipDefinition::bilingual("shared.path-completion", "输入 `@` 补全路径；文件名支持子串匹配，用 `Tab` 接受候选。", "Type `@` to complete paths; names support substring matching, and `Tab` accepts a candidate.", &[("Tab", S::MessageSuggest, A::Complete)], vec![]),
        TipDefinition::bilingual("shared.local-shell", "输入 `!` 执行本地命令，输出不会发送给模型。", "Use `!` to run a local command; its output is not sent to the model.", &[], vec![prerequisite(P::ShellAvailable)]),
        TipDefinition::bilingual("shared.skill-message", "用 `/skill:名称 消息` 调用技能，同时附上你的需求。", "Use `/skill:name message` to invoke a skill with your request.", &[], vec![command("skill"), prerequisite(P::Skills)]),
        TipDefinition::bilingual("shared.model-effort", "按 `Ctrl+L` 选择模型，按 `Ctrl+E` 调整思考强度。", "Press `Ctrl+L` to choose a model and `Ctrl+E` to adjust reasoning effort.", &[("Ctrl+L", S::Global, A::ChooseModel), ("Ctrl+E", S::Global, A::ChooseEffort)], vec![prerequisite(P::Models), prerequisite(P::CurrentEfforts)]),
        TipDefinition::bilingual("shared.model-marks", "在模型菜单用 `Shift+字母` 标记模型，之后按该字母快速切换。", "Use `Shift+letter` in the model menu to mark a model, then that letter to switch quickly.", &[], vec![prerequisite(P::Models), prerequisite(P::ModelLetters)]),
        TipDefinition::bilingual("shared.temporary-model", "用 `//<模型标记> 消息` 临时选择已标记模型，任务结束后恢复原模型。", "Use `//<model mark> message` for a marked model; the original model returns after the task.", &[], vec![prerequisite(P::ModelMark)]),
        TipDefinition::bilingual("shared.model-default-effort", "用 `/model 模型 set-default-effort 强度` 保存该模型的默认思考强度。", "Use `/model model set-default-effort effort` to save that model's default effort.", &[], vec![command("model"), prerequisite(P::AnyEfforts)]),
        TipDefinition::bilingual("shared.link-copy", "按 `Ctrl+Y`，再按标记字符，复制带有 ~n 的链接或路径。", "Press `Ctrl+Y`, then a tag character, to copy a link or path marked with ~n.", &[("Ctrl+Y", S::Global, A::CopyLink)], vec![prerequisite(P::QuickLinks)]),
        TipDefinition::bilingual("shared.mouse-copy", "用鼠标选中文字即可复制；阅读模式的 `y` 可复制完整内容块。", "Select text with the mouse to copy; Reading mode's `y` copies the whole block.", &[("y", S::ReadMode, A::CopyBlock)], vec![]),
        TipDefinition::bilingual("shared.settings-reload", "输入 `/settings` 调整前端设置，`/reload` 重载配置和后端资源。", "Use `/settings` for frontend options and `/reload` to reload config and backend resources.", &[], vec![command("settings"), command("reload")]),
        TipDefinition::bilingual("shared.config-path", "输入 `/econfig` 查看前端配置文件的位置。", "Use `/econfig` to see the frontend configuration file's location.", &[], vec![command("econfig")]),
        TipDefinition::bilingual("shared.new-resume", "空闲后用 `/new` 开新会话，用 `/resume` 续接历史会话。", "When idle, use `/new` for a new session or `/resume` to continue a saved one.", &[], vec![command("new"), command("resume")]),
        TipDefinition::bilingual("shared.compaction-queue", "`压缩期间`发送的普通消息会排队，等当前任务结束后再发送。", "Ordinary messages sent `during compaction` queue until the current task ends.", &[], vec![prerequisite(P::Compaction)]),
    ]
}
