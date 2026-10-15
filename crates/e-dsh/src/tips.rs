use e_tui::{
    tips::{TipDefinition, TipPrerequisite as P, TipRequirement as R},
    TuiApp,
};

pub fn definitions() -> Vec<TipDefinition> {
    vec![
        TipDefinition::bilingual("dshe.login-proxy", "空闲后在 `/login` 的 `Proxy` 页面管理自定义模型端点。", "When idle, manage custom model endpoints in `/login`'s `Proxy` page.", &[], vec![R::Command("login".into()), R::Capability("proxy-management".into()), R::Prerequisite(P::AttachedSession)]),
        TipDefinition::bilingual("dshe.new-mode", "空闲后用 `/new 模式` 按 DSH 预设新建会话，可用模式会出现在补全中。", "When idle, use `/new mode` for a DSH preset; available modes appear in completion.", &[], vec![R::Command("new".into()), R::Prerequisite(P::NewModes)]),
        TipDefinition::bilingual("dshe.compaction-route", "`/compact set-model 模型` 指定 DSH 手动和自动压缩使用的模型。", "Use `/compact set-model model` to choose the model for DSH manual and automatic compaction.", &[], vec![R::Command("compact".into()), R::Capability("compaction-routing".into()), R::Prerequisite(P::AttachedSession)]),
    ]
}

pub fn refresh_availability(app: &mut TuiApp) {
    let attached = app.session.session_id.is_some();
    for key in ["history", "proxy-management", "compaction-routing"] {
        app.tips.set_capability(key, attached);
    }
}
