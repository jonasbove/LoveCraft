//! Equations and calculations drawn in the editor (ProseCraft's `renderRichWidgets`): each is
//! rendered once in the text colour, at twice its size for a sharp texture.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use prosecraft_core::rich::{Pixels, RichKind, screen_pixels};

/// The editor's body size in points (ProseCraft sizes equations from it).
const BODY_PT: f64 = 12.75;
const SCALE: f64 = 2.0;

/// A drawing's size in points and its pixels, or why it can't be drawn.
pub type Drawn = Result<(egui::Vec2, Arc<Pixels>), String>;

fn hex(color: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// The drawing of an equation or calculation in `color` (kept for the session).
pub fn drawn(kind: RichKind, source: &str, color: egui::Color32) -> Drawn {
    type Key = (RichKind, String, [u8; 4]);
    static CACHE: OnceLock<Mutex<HashMap<Key, Drawn>>> = OnceLock::new();
    let key = (kind, source.to_owned(), color.to_array());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return found;
    }
    let result = screen_pixels(kind, source, BODY_PT, &hex(color), SCALE)
        .map(|(w, h, pixels)| (egui::vec2(w as f32, h as f32), Arc::new(pixels)));
    if let Ok(mut c) = cache.lock() {
        if c.len() > 400 {
            c.clear();
        }
        c.insert(key, result.clone());
    }
    result
}

/// The drawing's texture.
pub fn texture(ctx: &egui::Context, pixels: &Pixels) -> egui::TextureHandle {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.rgba.hash(&mut hasher);
    pixels.width.hash(&mut hasher);
    let id = egui::Id::unique(("rich-texture", hasher.finish()));
    if let Some(found) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return found;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [pixels.width as usize, pixels.height as usize],
        &pixels.rgba,
    );
    let texture = ctx.load_texture("rich", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// The kind of a block the editor keeps whole, when it is drawn.
pub fn block_kind(preview: &prosecraft_core::richtext::BlockPreview) -> Option<RichKind> {
    match preview.kind {
        prosecraft_core::richtext::BlockKind::Equation => Some(RichKind::MathBlock),
        prosecraft_core::richtext::BlockKind::Calculation => Some(RichKind::Calc),
        _ => None,
    }
}

/// A drawing's size in points (from the layout alone), or `None` when it can't be drawn.
pub fn size(kind: RichKind, source: &str) -> Option<egui::Vec2> {
    let options = prosecraft_core::rich::RenderOptions {
        png: false,
        color: String::new(),
        body_size_pt: BODY_PT,
    };
    prosecraft_core::rich::render(kind, source, &options)
        .ok()
        .map(|r| egui::vec2(r.width as f32, r.height as f32))
}

/// The space above and below a drawn block (`margin: 0.85em auto`).
pub const BLOCK_MARGIN: f32 = 12.0;
