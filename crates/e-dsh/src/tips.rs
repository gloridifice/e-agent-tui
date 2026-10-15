use e_tui::{
    tips::{TipDefinition, TipPrerequisite as P, TipRequirement as R},
    TuiApp,
};

pub fn definitions() -> Vec<TipDefinition> {
    vec![
        TipDefinition::bilingual(
            "dshe.login-proxy",
            "`/login` → `Proxy` 管理自定义模型端点。",
            "`/login` → `Proxy` to manage custom model endpoints.",
            &[],
            vec![
                R::Command("login".into()),
                R::Capability("proxy-management".into()),
                R::Prerequisite(P::AttachedSession),
            ],
        ),
        TipDefinition::bilingual(
            "dshe.new-mode",
            "`/new 模式` 按 DSH 预设新建会话。",
            "`/new mode` to start a session with a DSH preset.",
            &[],
            vec![R::Command("new".into()), R::Prerequisite(P::NewModes)],
        ),
        TipDefinition::bilingual(
            "dshe.new-mode-completion",
            "`/new` 补全候选列出可用模式。",
            "`/new` completion lists available modes.",
            &[],
            vec![R::Command("new".into()), R::Prerequisite(P::NewModes)],
        ),
        TipDefinition::bilingual(
            "dshe.compaction-route",
            "`/compact set-model 模型` 设置 DSH 手动和自动压缩的模型。",
            "`/compact set-model model` to set DSH's manual and automatic compaction model.",
            &[],
            vec![
                R::Command("compact".into()),
                R::Capability("compaction-routing".into()),
                R::Prerequisite(P::AttachedSession),
            ],
        ),
    ]
}

pub fn refresh_availability(app: &mut TuiApp) {
    let attached = app.session.session_id.is_some();
    for key in ["history", "proxy-management", "compaction-routing"] {
        app.tips.set_capability(key, attached);
    }
}
