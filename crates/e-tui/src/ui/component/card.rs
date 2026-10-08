use crate::{
    display::CardRole,
    theme::{Theme, ThemeStyle},
};

pub fn shell_style(theme: &Theme, role: CardRole) -> ThemeStyle {
    match role {
        CardRole::User => theme.card.user,
        CardRole::Skill | CardRole::Context => theme.card.context,
        CardRole::Detail => theme.card.detail,
        CardRole::Terminal => theme.surface.primary_text,
        CardRole::Attachment => theme.card.attachment,
    }
}
