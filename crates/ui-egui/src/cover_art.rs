//! Generated covers drawn (`renderGeneratedCover`): the ops [`prosecraft_core::publish::cover_art`]
//! lays out, filled on the CPU at 1600 × 2400 with vello_cpu, the letters as outlines from the
//! app's fonts.
//!
//! ProseCraft draws with the browser's fonts: Georgia (or the style's font) and Inter. Here the
//! serif is Source Serif 4, the sans Inter, and a monospace style font egui's Hack; without the
//! craft-fonts checkout, egui's own face stands in for all of them. Letters are placed by their
//! advances, without kerning.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use prosecraft_core::publish::cover::{self, CompiledCover, CoverOptions};
use prosecraft_core::publish::cover_art::{self, Align, CoverText, Font, Measure, Op, Paint, Rgba};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use vello_cpu::kurbo::{Affine, BezPath, Circle, Line, Rect, Shape};
use vello_cpu::peniko::{Color, Gradient};
use vello_cpu::{Pixmap, RenderContext, Resources};

/// The face for a canvas font: (name in the app's faces, egui's stand-in).
fn face_name(font: &Font) -> (&'static str, &'static str) {
    let family = font.family.to_lowercase();
    let heavy = font.weight >= 700;
    let medium = font.weight >= 500;
    if family.contains("monospace") || family.contains("courier") {
        return ("", "Hack");
    }
    if family.starts_with("inter") || family.contains("sans-serif") {
        let name = match () {
            () if font.italic => "Inter-Italic",
            () if heavy => "Inter-Bold",
            () if medium => "Inter-SemiBold",
            () => "Inter-Regular",
        };
        return (name, "Ubuntu-Light");
    }
    let name = match () {
        () if font.italic => "SourceSerif4-It",
        () if heavy => "SourceSerif4-Bold",
        () if medium => "SourceSerif4-Semibold",
        () => "SourceSerif4-Regular",
    };
    (name, "Ubuntu-Light")
}

/// egui's own faces, for when the app's aren't built in.
fn egui_face(name: &str) -> Option<&'static [u8]> {
    type Faces = HashMap<String, std::sync::Arc<dyn AsRef<[u8]> + Send + Sync>>;
    static FACES: OnceLock<Faces> = OnceLock::new();
    FACES
        .get_or_init(|| {
            egui::FontDefinitions::default()
                .font_data
                .into_iter()
                .map(|(name, data)| (name, data.font.clone()))
                .collect()
        })
        .get(name)
        .map(|data| (**data).as_ref())
}

fn face(font: &Font) -> Option<FontRef<'static>> {
    let (app, stand_in) = face_name(font);
    let bytes = crate::fonts::app_face(app).or_else(|| egui_face(stand_in))?;
    FontRef::new(bytes).ok()
}

/// Advances from the face (the canvas's `measureText`, without kerning).
struct Faces;

impl Measure for Faces {
    fn width(&self, text: &str, font: &Font) -> f64 {
        let Some(face) = face(font) else {
            return text.chars().count() as f64 * font.size * 0.5;
        };
        let size = Size::new(font.size as f32);
        let metrics = face.glyph_metrics(size, LocationRef::default());
        let charmap = face.charmap();
        text.chars()
            .map(|c| {
                let glyph = charmap.map(c).unwrap_or_default();
                f64::from(metrics.advance_width(glyph).unwrap_or(0.0))
            })
            .sum()
    }
}

/// A glyph's outline as a path, y flipped to the canvas's down.
struct Pen(BezPath);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), -f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), -f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(
            (f64::from(cx0), -f64::from(cy0)),
            (f64::from(x), -f64::from(y)),
        );
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to(
            (f64::from(cx0), -f64::from(cy0)),
            (f64::from(cx1), -f64::from(cy1)),
            (f64::from(x), -f64::from(y)),
        );
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

fn color(c: &Rgba) -> Color {
    let alpha = (c.alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::from_rgba8(c.rgb[0], c.rgb[1], c.rgb[2], alpha)
}

/// Text with its middle at `y` (`textBaseline = "middle"`: half way down the em box).
fn text(
    ctx: &mut RenderContext,
    line: &str,
    x: f64,
    y: f64,
    font: &Font,
    align: Align,
    fill: &Rgba,
) {
    let Some(face) = face(font) else {
        return;
    };
    let size = Size::new(font.size as f32);
    let metrics = face.metrics(size, LocationRef::default());
    let baseline = y + f64::from(metrics.ascent + metrics.descent) / 2.0;
    let width = Faces.width(line, font);
    let mut pen_x = if align == Align::Center {
        x - width / 2.0
    } else {
        x
    };
    let glyphs = face.glyph_metrics(size, LocationRef::default());
    let outlines = face.outline_glyphs();
    let charmap = face.charmap();
    ctx.set_paint(color(fill));
    for c in line.chars() {
        let glyph = charmap.map(c).unwrap_or_default();
        if let Some(outline) = outlines.get(glyph) {
            let mut pen = Pen(BezPath::new());
            let settings = DrawSettings::unhinted(size, LocationRef::default());
            if outline.draw(settings, &mut pen).is_ok() {
                ctx.set_transform(Affine::translate((pen_x, baseline)));
                ctx.fill_path(&pen.0);
                ctx.reset_transform();
            }
        }
        pen_x += f64::from(glyphs.advance_width(glyph).unwrap_or(0.0));
    }
}

/// The ops filled into RGBA pixels.
pub(crate) fn rasterize(ops: &[Op]) -> Vec<u8> {
    let (w, h) = (cover::WIDTH as u16, cover::HEIGHT as u16);
    let mut ctx = RenderContext::new(w, h);
    for op in ops {
        match op {
            Op::FillRect {
                x,
                y,
                width,
                height,
                paint,
            } => {
                match paint {
                    Paint::Solid(c) => ctx.set_paint(color(c)),
                    Paint::Vertical(a, b) => ctx.set_paint(
                        Gradient::new_linear((0.0, 0.0), (0.0, f64::from(h)))
                            .with_stops([(0.0, color(a)), (1.0, color(b))].as_slice()),
                    ),
                }
                ctx.fill_rect(&Rect::new(*x, *y, x + width, y + height));
            }
            Op::StrokeRect {
                x,
                y,
                width,
                height,
                color: c,
                line,
            } => {
                ctx.set_paint(color(c));
                ctx.set_stroke(vello_cpu::kurbo::Stroke::new(*line));
                ctx.stroke_rect(&Rect::new(*x, *y, x + width, y + height));
            }
            Op::Line {
                from,
                to,
                color: c,
                line,
            } => {
                ctx.set_paint(color(c));
                ctx.set_stroke(vello_cpu::kurbo::Stroke::new(*line));
                ctx.stroke_path(&Line::new(*from, *to).to_path(0.1));
            }
            Op::Circle {
                x,
                y,
                radius,
                color: c,
            } => {
                ctx.set_paint(color(c));
                ctx.fill_path(&Circle::new((*x, *y), *radius).to_path(0.1));
            }
            Op::Text {
                text: line,
                x,
                y,
                font,
                align,
                color: c,
            } => text(&mut ctx, line, *x, *y, font, *align, c),
        }
    }
    let mut pixmap = Pixmap::new(w, h);
    let mut resources = Resources::new();
    ctx.flush();
    ctx.render(&mut pixmap, &mut resources);
    pixmap
        .data()
        .iter()
        .flat_map(|p| {
            let un = |c: u8| {
                if p.a == 0 {
                    0
                } else {
                    ((u16::from(c) * 255 + u16::from(p.a) / 2) / u16::from(p.a)).min(255) as u8
                }
            };
            [un(p.r), un(p.g), un(p.b), p.a]
        })
        .collect()
}

/// `encodeGeneratedCover`: PNG, or WebP at 0.92 when the PNG is too large (else the PNG).
fn encode(rgba: Vec<u8>) -> Option<String> {
    use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};
    let image = Image::from_u8(cover::WIDTH, cover::HEIGHT, ChannelLayout::Rgba, rgba).ok()?;
    let url = |media: &str, bytes: &[u8]| {
        format!(
            "data:{media};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    };
    let png = url(
        "image/png",
        &craft_image_codecs::encode(&image, Format::Png, &EncodeOptions::default()).ok()?,
    );
    if prosecraft_core::html::sanitize::is_embedded_image_src(&png) {
        return Some(png);
    }
    let options = EncodeOptions {
        webp_lossless: false,
        webp_quality: 92,
        ..EncodeOptions::default()
    };
    if let Ok(webp) = craft_image_codecs::encode(&image, Format::WebP, &options) {
        let webp = url("image/webp", &webp);
        if prosecraft_core::html::sanitize::is_embedded_image_src(&webp) {
            return Some(webp);
        }
    }
    Some(png)
}

/// `generatedCoverCache`: the last dozen covers by what they show (`generatedCoverKey`).
fn cache() -> &'static Mutex<Vec<(String, CompiledCover)>> {
    static CACHE: OnceLock<Mutex<Vec<(String, CompiledCover)>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// `resolveCoverImage` for a generated cover: drawn once for each title, subtitle, author,
/// design, palette and style font.
pub(crate) fn generated(
    book: &CoverText<'_>,
    options: &CoverOptions,
    style_font: &str,
) -> Option<CompiledCover> {
    if options.mode != "generated" {
        return None;
    }
    let key = format!(
        "{:?}",
        (
            book.title,
            book.subtitle,
            book.author,
            options.design,
            options.palette,
            style_font
        )
    );
    if let Some((_, found)) = cache().lock().ok()?.iter().find(|(k, _)| *k == key) {
        return Some(found.clone());
    }
    let palette = cover::palette_for(options.palette);
    let ops = cover_art::draw(book, options.design, palette, style_font, &Faces);
    let data_url = encode(rasterize(&ops))?;
    let compiled = CompiledCover {
        media_type: cover::media_type(&data_url),
        data_url,
        width: cover::WIDTH,
        height: cover::HEIGHT,
        background: palette.background.to_owned(),
    };
    if let Ok(mut cache) = cache().lock() {
        cache.push((key, compiled.clone()));
        if cache.len() > 12 {
            cache.remove(0);
        }
    }
    Some(compiled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_cover_is_drawn_on_its_palette() {
        let text = CoverText {
            title: "The Lighthouse Keeper",
            subtitle: "",
            author: "A. Writer",
        };
        let options = CoverOptions {
            mode: "generated",
            design: "bold",
            palette: "clay",
            ..CoverOptions::default()
        };
        let compiled = generated(&text, &options, "Georgia, serif").expect("a cover");
        assert_eq!((compiled.width, compiled.height), (1600, 2400));
        assert_eq!(compiled.background, "#a84e38");
        assert!(prosecraft_core::html::sanitize::is_embedded_image_src(
            &compiled.data_url
        ));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(compiled.data_url.split_once(',').expect("data").1)
            .expect("base64");
        let image = craft_image_codecs::decode(&bytes).expect("decoded");
        assert_eq!(image.dimensions(), (1600, 2400));
        let rgba = image.to_rgba8();
        let at = |x: usize, y: usize| {
            let p = (y * 1600 + x) * 4;
            [rgba[p], rgba[p + 1], rgba[p + 2], rgba[p + 3]]
        };
        // The accent bar across the top, the background below it, the author block at the foot.
        assert_eq!(at(800, 5), [0xf0, 0xc1, 0x9d, 255]);
        // The background darkens from #a84e38 to #8d422f (16% darker) down the page.
        let (top, low) = (at(40, 100), at(40, 1600));
        assert!((165..=168).contains(&top[0]), "{top:?}");
        assert!((147..=153).contains(&low[0]), "{low:?}");
        assert_eq!(at(1500, 2300), [0xf0, 0xc1, 0x9d, 255]);
        // The title's letters are drawn in the ink colour somewhere in its block.
        let inked = (700..1100)
            .flat_map(|y| (140..1400).map(move |x| (x, y)))
            .filter(|&(x, y)| at(x, y) == [0xff, 0xf1, 0xe3, 255])
            .count();
        assert!(inked > 10_000, "{inked} ink pixels");
        // Drawn once: the second ask is the same cover.
        assert_eq!(generated(&text, &options, "Georgia, serif"), Some(compiled));
        assert_eq!(
            generated(&text, &CoverOptions::default(), "Georgia, serif"),
            None
        );
    }
}
