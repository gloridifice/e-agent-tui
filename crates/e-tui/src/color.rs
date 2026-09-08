use ratatui::style::Color;

pub(crate) fn lerp_rgb(from: Color, to: Color, weight: f64) -> Color {
    let weight = weight.clamp(0.0, 1.0);
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let channel = |a: u8, b: u8| {
                (f64::from(a) + (f64::from(b) - f64::from(a)) * weight).round() as u8
            };
            Color::Rgb(channel(r1, r2), channel(g1, g2), channel(b1, b2))
        }
        _ => to,
    }
}
