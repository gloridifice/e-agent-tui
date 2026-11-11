//! Terminal-owned math overlays; semantic layouts and snapshots remain text-only.

use std::{cell::RefCell, collections::VecDeque, io::IsTerminal};

use ab_glyph::Font;
use image::{DynamicImage, Rgba, RgbaImage};
use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    style::Color,
    widgets::Widget,
};
use ratatui_image::{
    picker::{cap_parser::QueryStdioOptions, Capability, Picker, ProtocolType},
    protocol::{sixel::Sixel, Protocol},
    FontSize, Image,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MathImage {
    pub(crate) source: std::sync::Arc<str>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) foreground: Color,
}

const MAX_ENTRIES: usize = 64;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 1024 * 1024;
const MAX_NEW_PER_FRAME: usize = 2;
const MAX_PLACEMENTS: usize = 64;

thread_local! {
    static FRAME: RefCell<Option<Vec<Placement>>> = const { RefCell::new(None) };
}

pub(crate) struct FrameImages;

impl FrameImages {
    pub(crate) fn begin(enabled: bool) -> Self {
        FRAME.with(|frame| *frame.borrow_mut() = enabled.then(Vec::new));
        Self
    }

    pub(crate) fn take(&self) -> Vec<Placement> {
        FRAME.with(|frame| frame.borrow_mut().take().unwrap_or_default())
    }
}

impl Drop for FrameImages {
    fn drop(&mut self) {
        FRAME.with(|frame| *frame.borrow_mut() = None);
    }
}

pub(crate) struct Placement {
    image: MathImage,
    background: Color,
    underlay: Buffer,
}

pub(crate) fn enabled() -> bool {
    FRAME.with(|frame| frame.borrow().is_some())
}

pub(crate) fn offer(image: &MathImage, area: Rect, background: Color, buffer: &Buffer) {
    if area.width != image.width
        || area.height != image.height
        || area.intersection(buffer.area) != area
    {
        return;
    }
    FRAME.with(|frame| {
        let mut frame = frame.borrow_mut();
        let Some(placements) = frame.as_mut().filter(|items| items.len() < MAX_PLACEMENTS) else {
            return;
        };
        let mut underlay = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                underlay[(x, y)] = buffer[(x, y)].clone();
            }
        }
        placements.push(Placement {
            image: image.clone(),
            background,
            underlay,
        });
    });
}

pub(crate) fn suppress(area: Rect) {
    FRAME.with(|frame| {
        if let Some(placements) = frame.borrow_mut().as_mut() {
            placements.retain(|p| p.underlay.area.intersection(area).is_empty());
        }
    });
}

struct Entry {
    image: MathImage,
    background: Color,
    protocol: Option<Protocol>,
    bytes: usize,
}

pub(crate) struct MathGraphics {
    picker: Picker,
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl MathGraphics {
    /// Called only by TerminalOwner, before any input reader starts.
    pub(crate) fn detect() -> Option<Self> {
        if !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
            || std::env::var_os("TMUX").is_some()
            || std::env::var("TERM")
                .is_ok_and(|term| term.starts_with("tmux") || term.starts_with("screen"))
        {
            return None;
        }
        let picker = Picker::from_query_stdio_with_options(QueryStdioOptions {
            timeout_ms: 500,
            blacklist_protocols: vec![ProtocolType::Kitty, ProtocolType::Iterm2],
            ..Default::default()
        })
        .ok()?;
        if picker.protocol_type() != ProtocolType::Sixel
            || !picker.capabilities().contains(&Capability::Sixel)
        {
            return None;
        }
        let font = picker.font_size();
        if font.width == 0 || font.height == 0 || font.width > 128 || font.height > 256 {
            return None;
        }
        Some(Self {
            picker,
            entries: VecDeque::new(),
            bytes: 0,
        })
    }

    pub(crate) fn paint(&mut self, placements: Vec<Placement>, buffer: &mut Buffer) {
        let mut new = 0;
        for placement in placements {
            let area = placement.underlay.area;
            if area.intersection(buffer.area) != area
                || (area.top()..area.bottom()).any(|y| {
                    (area.left()..area.right())
                        .any(|x| buffer[(x, y)] != placement.underlay[(x, y)])
                })
            {
                continue;
            }
            let index = self.entries.iter().position(|entry| {
                entry.image == placement.image && entry.background == placement.background
            });
            let entry = if let Some(index) = index {
                self.entries.remove(index).expect("located cache entry")
            } else {
                if new == MAX_NEW_PER_FRAME {
                    continue;
                }
                new += 1;
                let protocol = std::panic::catch_unwind(|| {
                    raster(
                        &placement.image,
                        placement.background,
                        self.picker.font_size(),
                    )
                    .and_then(|pixels| {
                        Sixel::new(
                            pixels,
                            Size::new(area.width, area.height),
                            self.picker.tmux_detected(),
                        )
                        .ok()
                    })
                })
                .ok()
                .flatten()
                .filter(|protocol| protocol.data.len() <= MAX_CACHE_BYTES);
                let bytes = protocol.as_ref().map_or(0, |protocol| protocol.data.len())
                    + placement.image.source.len();
                self.bytes += bytes;
                Entry {
                    image: placement.image,
                    background: placement.background,
                    protocol: protocol.map(Protocol::Sixel),
                    bytes,
                }
            };
            if let Some(protocol) = &entry.protocol {
                Image::new(protocol).render(area, buffer);
            }
            self.entries.push_back(entry);
            while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_CACHE_BYTES {
                if let Some(evicted) = self.entries.pop_front() {
                    self.bytes -= evicted.bytes;
                }
            }
        }
    }
}

fn rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

fn raster(image: &MathImage, background: Color, cell: FontSize) -> Option<DynamicImage> {
    use ratex_types::{
        color::Color as MathColor, display_item::DisplayItem, math_style::MathStyle,
    };
    if image.source.len() > 1024 || image.width == 0 || image.height == 0 {
        return None;
    }
    let (r, g, b) = rgb(image.foreground)?;
    let (br, bg, bb) = rgb(background)?;
    let width = u32::from(image.width) * u32::from(cell.width);
    let height = u32::from(image.height) * u32::from(cell.height);
    if u64::from(width) * u64::from(height) > MAX_PIXELS || width < 8 || height < 8 {
        return None;
    }
    let ast = ratex_parser::parse(&image.source).ok()?;
    let layout = ratex_layout::layout(
        &ast,
        &ratex_layout::LayoutOptions {
            style: MathStyle::Display,
            color: MathColor::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
            ..Default::default()
        },
    );
    ratex_layout::validate_layout(&layout).ok()?;
    let display = ratex_layout::to_display_list(&layout);
    if display.items.len() > 1024
        || !display.width.is_finite()
        || !display.total_height().is_finite()
        || display.width <= 0.0
        || display.total_height() <= 0.0
    {
        return None;
    }

    // Never let the rasterizer resolve system fonts for missing embedded glyphs.
    for item in &display.items {
        if let DisplayItem::GlyphPath { font, .. } = item {
            let id = ratex_font::FontId::parse(font)?;
            if matches!(
                id,
                ratex_font::FontId::CjkRegular
                    | ratex_font::FontId::CjkFallback
                    | ratex_font::FontId::EmojiFallback
            ) {
                return None;
            }
        }
    }
    let fonts = ratex_font_loader::load_fonts_for_items_parsed("", &display.items).ok()?;
    for item in &display.items {
        if let DisplayItem::GlyphPath {
            font, char_code, ..
        } = item
        {
            let id = ratex_font::FontId::parse(font)?;
            let font = fonts.get(&id)?;
            let ch = ratex_font::katex_ttf_glyph_char(id, *char_code);
            let glyph = font.glyph_id(ch).with_scale(20.0);
            font.outline_glyph(glyph)?;
        }
    }
    let font_size = ((width - 4) as f64 / display.width)
        .min((height - 4) as f64 / display.total_height())
        .min(f64::from(cell.height) * 1.2) as f32;
    if !font_size.is_finite() || font_size < 8.0 {
        return None;
    }
    let png = ratex_render::render_to_png(
        &display,
        &ratex_render::RenderOptions {
            font_size,
            padding: 1.0,
            background_color: MathColor::rgb(
                br as f32 / 255.0,
                bg as f32 / 255.0,
                bb as f32 / 255.0,
            ),
            ..Default::default()
        },
    )
    .ok()?;
    let pixels = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?;
    if pixels.width() > width || pixels.height() > height {
        return None;
    }
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([br, bg, bb, 255]));
    image::imageops::overlay(
        &mut canvas,
        &pixels.to_rgba8(),
        0,
        i64::from((height - pixels.height()) / 2),
    );
    Some(DynamicImage::ImageRgba8(canvas))
}
