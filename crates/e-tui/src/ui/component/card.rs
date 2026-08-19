use crate::{
    display::CardRole,
    theme::{Theme, ThemeStyle},
};

pub fn shell_style(theme: &Theme, role: CardRole) -> ThemeStyle {
    match role {
        CardRole::User => theme.card.user,
        CardRole::Context => theme.card.context,
        CardRole::Detail => theme.card.detail,
        CardRole::Attachment => theme.card.attachment,
    }
}
