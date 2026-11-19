//! Pictures in the text: each embedded image's size and layout, read once, and its texture.
//!
//! A picture's `<img>` carries its whole file as a data URL, so it is read only when it is new:
//! the facts the layout needs are kept by a hash of the markup, and the texture in egui's memory
//! under the same hash.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use prosecraft_core::html::image_layout::{self, ImageLayout};

/// What the editor needs to lay out and draw a picture.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PictureInfo {
    pub key: u64,
    /// The image's pixels (`naturalWidth`, `naturalHeight`).
    pub size: (u32, u32),
    pub layout: ImageLayout,
    pub alt: String,
    /// Its `width` attribute (ProseCraft's Image size buttons set it), in CSS pixels.
    pub pixel_width: Option<u32>,
}

impl PictureInfo {
    /// The picture's width as a share of the column, as ProseCraft's CSS sizes it
    /// (`imageLayoutStyle`): a wrapped picture takes at most half.
    pub fn share(&self) -> f32 {
        let percent = match self.layout.size {
            "small" => 25,
            "medium" => 50,
            "full" => 100,
            "custom" => self.layout.width.unwrap_or(100),
            _ => 75,
        };
        let percent = if self.layout.wrap == "block" {
            percent
        } else {
            percent.min(50)
        };
        percent as f32 / 100.0
    }

    /// Its drawn size in a column `width` points wide.
    pub fn drawn(&self, width: f32) -> egui::Vec2 {
        let w = match self.pixel_width {
            // A picture given a pixel width at the default column size keeps it, within the
            // column (`max-width: 100%`).
            Some(px)
                if self.layout.size == image_layout::DEFAULT.size
                    && self.layout.wrap == "block" =>
            {
                (px as f32).min(width)
            }
            _ => width * self.share(),
        }
        .max(1.0);
        let h = w * self.size.1 as f32 / self.size.0.max(1) as f32;
        egui::vec2(w, h)
    }
}

/// The space above and below a picture in the text (`.editor-content img { margin: 18px 0 }`).
pub(crate) const MARGIN: f32 = 18.0;

fn key(html: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    html.hash(&mut hasher);
    hasher.finish()
}

fn image_bytes(src: &str) -> Option<Vec<u8>> {
    let data = ["png", "jpeg", "webp"]
        .iter()
        .find_map(|k| src.strip_prefix(&format!("data:image/{k};base64,")))?;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

fn first_image(html: &str) -> Option<prosecraft_core::html::Element> {
    prosecraft_core::html::parse_fragment(html)
        .into_iter()
        .find_map(|n| match n {
            prosecraft_core::html::Node::Element(e) if e.name == "img" => Some(e),
            _ => None,
        })
}

/// Pictures read so far (a few hundred at most are kept).
fn known() -> &'static Mutex<HashMap<u64, Option<PictureInfo>>> {
    static KNOWN: OnceLock<Mutex<HashMap<u64, Option<PictureInfo>>>> = OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

/// A picture's size, layout and alternative text, from its `<img>`; `None` when the image
/// can't be read.
pub(crate) fn info(html: &str) -> Option<PictureInfo> {
    let key = key(html);
    if let Some(found) = known().lock().ok().and_then(|k| k.get(&key).cloned()) {
        return found;
    }
    let read = first_image(html).and_then(|image| {
        let bytes = image_bytes(image.attr("src")?)?;
        let size = craft_image_codecs::probe_size(&bytes).ok()?;
        Some(PictureInfo {
            key,
            size,
            layout: image_layout::read(&image),
            alt: image.attr("alt").unwrap_or_default().to_owned(),
            pixel_width: image.attr("width").and_then(|w| w.trim().parse().ok()),
        })
    });
    if let Ok(mut known) = known().lock() {
        if known.len() > 512 {
            known.clear();
        }
        known.insert(key, read.clone());
    }
    read
}

/// An image's pixels for a texture, scaled down (by averaging) when a side is longer than the
/// graphics card takes: a picture or cover is up to 2400 pixels tall, more than some allow.
pub(crate) fn color_image(
    ctx: &egui::Context,
    image: &craft_image_codecs::Image,
) -> egui::ColorImage {
    let (w, h) = image.dimensions();
    let (w, h) = (w as usize, h as usize);
    let rgba = image.to_rgba8();
    let max = ctx.input(|i| i.max_texture_side).max(1);
    let factor = w.max(h).div_ceil(max).max(1);
    if factor == 1 {
        return egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
    }
    let (tw, th) = (w.div_ceil(factor), h.div_ceil(factor));
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0u32; 4];
            let mut count = 0;
            for y in ty * factor..((ty + 1) * factor).min(h) {
                for x in tx * factor..((tx + 1) * factor).min(w) {
                    let p = (y * w + x) * 4;
                    for c in 0..4 {
                        sum[c] += u32::from(rgba[p + c]);
                    }
                    count += 1;
                }
            }
            let q = (ty * tw + tx) * 4;
            for c in 0..4 {
                out[q + c] = (sum[c] / count.max(1)) as u8;
            }
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([tw, th], &out)
}

/// The picture's texture, decoded the first time it is drawn.
pub(crate) fn texture(
    ctx: &egui::Context,
    html: &str,
    info: &PictureInfo,
) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique(("picture", info.key));
    if let Some(found) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return found;
    }
    let texture = first_image(html)
        .and_then(|image| image_bytes(image.attr("src")?))
        .and_then(|bytes| craft_image_codecs::decode(&bytes).ok())
        .map(|image| {
            ctx.load_texture(
                format!("picture-{:x}", info.key),
                color_image(ctx, &image),
                egui::TextureOptions::LINEAR,
            )
        });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tall_picture_fits_the_largest_texture() {
        let ctx = egui::Context::default();
        let max = ctx.input(|i| i.max_texture_side);
        let image = craft_image_codecs::Image::from_u8(
            10,
            2400,
            craft_image_codecs::ChannelLayout::Rgba,
            vec![120; 10 * 2400 * 4],
        )
        .expect("image");
        let pixels = super::color_image(&ctx, &image);
        assert!(
            pixels.size[0] <= max && pixels.size[1] <= max,
            "{:?} > {max}",
            pixels.size
        );
        assert_eq!(pixels.size, [5, 1200]);
        assert_eq!(
            pixels.pixels[0],
            egui::Color32::from_rgba_unmultiplied(120, 120, 120, 120)
        );
    }
}
