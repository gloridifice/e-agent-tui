use e_tui::tips::{TipDefinition, TipPrerequisite as P, TipRequirement as R};

pub fn definitions() -> Vec<TipDefinition> {
    vec![
        TipDefinition::bilingual("pie.fork", "空闲后用 `/fork` 从较早的用户消息分叉，原输入会回到编辑栏。", "When idle, `/fork` branches from an earlier user message and restores its input to the composer.", &[], vec![R::Command("fork".into()), R::Prerequisite(P::UserMessages)]),
        TipDefinition::bilingual("pie.clone", "空闲后用 `/clone` 复制当前会话分支，不会创建 Git 分支。", "When idle, `/clone` duplicates the current session branch, not a Git branch.", &[], vec![R::Command("clone".into()), R::Prerequisite(P::AttachedSession)]),
        TipDefinition::bilingual("pie.native-auth", "空闲后用 `/login 提供商` 登录，用 `/logout` 移除已保存的凭据。", "When idle, use `/login provider` to sign in and `/logout` to remove saved credentials.", &[], vec![R::Command("login".into()), R::Command("logout".into()), R::Capability("native-auth".into())]),
        TipDefinition::bilingual("pie.mcp", "用 `/mcp` 管理 MCP 服务和工具；工作时保存配置后，空闲再 `/reload`。", "Use `/mcp` to manage MCP servers and tools; after saving while working, `/reload` when idle.", &[], vec![R::Command("mcp".into()), R::Command("reload".into()), R::Prerequisite(P::NativeMcp)]),
        TipDefinition::bilingual("pie.resume-tree", "在 `/resume` 中，`(fork)` 标记表示从父会话分叉出的历史会话。", "In `/resume`, `(fork)` marks a saved session branched from its parent.", &[], vec![R::Command("resume".into()), R::Capability("session-ancestry".into())]),
        TipDefinition::bilingual("pie.resume-cli", "退出 pie 后，终端会显示当前已保存会话的 `恢复命令`。", "After exiting pie, the terminal shows a `resume command` for the current saved session.", &[], vec![R::Capability("saved-session".into())]),
        TipDefinition::bilingual("pie.error-recovery", "可在 `/settings` 关闭 `错误自动重试`，取消等待中的追加重试。", "Turn off `Retry model errors` in `/settings` to cancel waiting additional retries.", &[], vec![R::Command("settings".into()), R::Prerequisite(P::ErrorRecovery)]),
        TipDefinition::bilingual("pie.auto-compaction", "Pi 的`自动压缩`始终使用原生默认模型；指定压缩模型只影响手动压缩。", "Pi's `automatic compaction` uses its native default model; a compaction override only affects manual compaction.", &[], vec![R::Capability("native-compaction".into())]),
    ]
}
