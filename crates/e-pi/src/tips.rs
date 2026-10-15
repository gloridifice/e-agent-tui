use e_tui::tips::{TipDefinition, TipPrerequisite as P, TipRequirement as R};

pub fn definitions() -> Vec<TipDefinition> {
    vec![
        TipDefinition::bilingual(
            "pie.fork",
            "`/fork` 从较早的用户消息分叉。",
            "`/fork` to branch from an earlier user message.",
            &[],
            vec![R::Command("fork".into()), R::Prerequisite(P::UserMessages)],
        ),
        TipDefinition::bilingual(
            "pie.fork-input",
            "`/fork` 会将选中的原消息放回输入框。",
            "`/fork` restores the selected message to the composer.",
            &[],
            vec![R::Command("fork".into()), R::Prerequisite(P::UserMessages)],
        ),
        TipDefinition::bilingual(
            "pie.clone",
            "`/clone` 复制当前会话分支，不创建 Git 分支。",
            "`/clone` to copy the current session branch, not a Git branch.",
            &[],
            vec![
                R::Command("clone".into()),
                R::Prerequisite(P::AttachedSession),
            ],
        ),
        TipDefinition::bilingual(
            "pie.native-auth",
            "`/login 提供商` 登录。",
            "`/login provider` to sign in.",
            &[],
            vec![
                R::Command("login".into()),
                R::Capability("native-auth".into()),
            ],
        ),
        TipDefinition::bilingual(
            "pie.logout",
            "`/logout` 移除已保存的凭据。",
            "`/logout` to remove saved credentials.",
            &[],
            vec![
                R::Command("logout".into()),
                R::Capability("native-auth".into()),
            ],
        ),
        TipDefinition::bilingual(
            "pie.mcp",
            "`/mcp` 管理 MCP 服务和工具。",
            "`/mcp` to manage MCP servers and tools.",
            &[],
            vec![R::Command("mcp".into()), R::Prerequisite(P::NativeMcp)],
        ),
        TipDefinition::bilingual(
            "pie.mcp-reload",
            "`/reload` 应用已保存的 MCP 配置。",
            "`/reload` to apply saved MCP config.",
            &[],
            vec![R::Command("reload".into()), R::Prerequisite(P::NativeMcp)],
        ),
        TipDefinition::bilingual(
            "pie.resume-cli",
            "退出 pie 后，终端会显示当前会话的`恢复命令`。",
            "Exiting pie prints a `resume command` for the current session.",
            &[],
            vec![R::Capability("saved-session".into())],
        ),
        TipDefinition::bilingual(
            "pie.error-recovery",
            "`/settings` 关闭`错误自动重试`，取消等待中的追加重试。",
            "`/settings` → `Retry model errors`: off cancels pending extra retries.",
            &[],
            vec![
                R::Command("settings".into()),
                R::Prerequisite(P::ErrorRecovery),
            ],
        ),
        TipDefinition::bilingual(
            "pie.auto-compaction",
            "Pi 的`自动压缩`始终采用原生默认模型。",
            "Pi's `automatic compaction`: always the native default model.",
            &[],
            vec![R::Capability("native-compaction".into())],
        ),
        TipDefinition::bilingual(
            "pie.manual-compaction-model",
            "Pi 的压缩模型设置只影响手动压缩。",
            "Pi's compaction model override affects manual compaction only.",
            &[],
            vec![R::Capability("native-compaction".into())],
        ),
    ]
}
