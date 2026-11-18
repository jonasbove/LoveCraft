//! Generated covers (`renderGeneratedCover` and the `draw…Cover` functions in ProseCraft's
//! `src/lib/publish/cover.ts`): what each design draws on the 1600 × 2400 canvas, as a list of
//! [`Op`]s. Text is fitted with a [`Measure`] the caller gives, so the layout is ProseCraft's
//! step for step and the app draws the ops with its own fonts.

use crate::html::is_js_whitespace;

use super::cover::{CoverPalette, HEIGHT, WIDTH};

/// `COVER_SANS_FONT`: Inter first.
pub const SANS: &str =
    "Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif";

/// A colour with an alpha, as a canvas takes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub rgb: [u8; 3],
    pub alpha: f64,
}

impl Rgba {
    /// `#rrggbb`, opaque (`hexToRgb`).
    pub fn hex(hex: &str) -> Self {
        let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
        Self {
            rgb: [(value >> 16) as u8, (value >> 8) as u8, value as u8],
            alpha: 1.0,
        }
    }

    /// `withAlpha`.
    pub fn with_alpha(hex: &str, alpha: f64) -> Self {
        Self {
            alpha,
            ..Self::hex(hex)
        }
    }

    /// As the canvas states it: `#rrggbb` when opaque, else `rgba(r, g, b, a)`.
    pub fn css(&self) -> String {
        let [r, g, b] = self.rgb;
        if self.alpha >= 1.0 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!(
                "rgba({r}, {g}, {b}, {})",
                crate::js::number_to_string(self.alpha)
            )
        }
    }
}

/// `Math.round`.
fn math_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `shade`: the colour moved toward black (`amount` < 0) or white.
pub fn shade(hex: &str, amount: f64) -> String {
    let Rgba { rgb, .. } = Rgba::hex(hex);
    let target = if amount < 0.0 { 0.0 } else { 255.0 };
    let mix = amount.abs();
    let channel = |c: u8| {
        let c = f64::from(c);
        math_round(c + (target - c) * mix).clamp(0.0, 255.0) as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgb[0]),
        channel(rgb[1]),
        channel(rgb[2])
    )
}

/// The canvas font of a run of text (`setCanvasFont`): the style's family or Inter, a weight,
/// italic or not, and the size in pixels, rounded.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub size: f64,
}

impl Font {
    fn new(size: f64, family: &str, weight: u16, italic: bool) -> Self {
        Self {
            family: family.to_owned(),
            weight,
            italic,
            size: math_round(size),
        }
    }

    /// `context.font`.
    pub fn css(&self) -> String {
        format!(
            "{} {} {}px {}",
            if self.italic { "italic" } else { "normal" },
            self.weight,
            crate::js::number_to_string(self.size),
            self.family
        )
    }
}

/// `context.measureText(text).width`.
pub trait Measure {
    fn width(&self, text: &str, font: &Font) -> f64;
}

/// Where a line of text is anchored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// A fill: a colour, or the background's top-to-bottom gradient.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    /// `createLinearGradient(0, 0, 0, 2400)` from the first colour to the second.
    Vertical(Rgba, Rgba),
}

/// One drawing call, in canvas pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    FillRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        paint: Paint,
    },
    StrokeRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: Rgba,
        line: f64,
    },
    Line {
        from: (f64, f64),
        to: (f64, f64),
        color: Rgba,
        line: f64,
    },
    Circle {
        x: f64,
        y: f64,
        radius: f64,
        color: Rgba,
    },
    /// Text whose middle (`textBaseline = "middle"`) is at `y`.
    Text {
        text: String,
        x: f64,
        y: f64,
        font: Font,
        align: Align,
        color: Rgba,
    },
}

/// The canvas being drawn on: the calls so far, and the font it is set to. As on a canvas, the
/// font stays as last set: `drawFittedText` doesn't set it, so a Classic or Minimal title with a
/// subtitle is drawn in the subtitle's fitted font, as ProseCraft draws it
/// (jennraeross/ProseCraft#61; the fix, #62, sets the font in `drawFittedText`).
struct Canvas<'a> {
    ops: Vec<Op>,
    font: Font,
    measure: &'a dyn Measure,
}

impl Canvas<'_> {
    /// `setCanvasFont`.
    fn set_font(&mut self, size: f64, family: &str, weight: u16, italic: bool) {
        self.font = Font::new(size, family, weight, italic);
    }

    /// `measureText` in the current font.
    fn width(&self, text: &str) -> f64 {
        self.measure.width(text, &self.font)
    }

    fn line(&mut self, from: (f64, f64), to: (f64, f64), color: Rgba, width: f64) {
        self.ops.push(Op::Line {
            from,
            to,
            color,
            line: width,
        });
    }

    fn solid(&mut self, (x, y, width, height): (f64, f64, f64, f64), color: Rgba) {
        self.ops.push(Op::FillRect {
            x,
            y,
            width,
            height,
            paint: Paint::Solid(color),
        });
    }

    /// `drawBackground`.
    fn background(&mut self, palette: &CoverPalette) {
        self.ops.push(Op::FillRect {
            x: 0.0,
            y: 0.0,
            width: f64::from(WIDTH),
            height: f64::from(HEIGHT),
            paint: Paint::Vertical(
                Rgba::hex(palette.background),
                Rgba::hex(&shade(palette.background, -0.16)),
            ),
        });
    }

    /// `wrapText`.
    fn wrap(&self, text: &str, max_width: f64) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let words: Vec<&str> = paragraph
                .split(is_js_whitespace)
                .filter(|w| !w.is_empty())
                .collect();
            if words.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut line = String::new();
            for word in words {
                let candidate = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if line.is_empty() || self.width(&candidate) <= max_width {
                    line = candidate;
                } else {
                    lines.push(std::mem::replace(&mut line, word.to_owned()));
                }
            }
            if !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    /// `fitText`: the largest size (stepping down by 4) at which the text fits.
    #[allow(clippy::too_many_arguments)]
    fn fit(
        &mut self,
        text: &str,
        family: &str,
        weight: u16,
        italic: bool,
        (max_width, max_height): (f64, f64),
        (max_size, min_size): (f64, f64),
        max_lines: usize,
        line_ratio: f64,
    ) -> Fitted {
        if text.trim_matches(is_js_whitespace).is_empty() {
            return Fitted {
                lines: Vec::new(),
                line_height: 0.0,
            };
        }
        let mut size = max_size;
        while size >= min_size {
            self.set_font(size, family, weight, italic);
            let lines = self.wrap(text, max_width);
            let line_height = size * line_ratio;
            let widest = lines.iter().map(|l| self.width(l)).fold(0.0, f64::max);
            if widest <= max_width + 0.5
                && lines.len() <= max_lines
                && lines.len() as f64 * line_height <= max_height
            {
                return Fitted { lines, line_height };
            }
            size -= 4.0;
        }
        self.set_font(min_size, family, weight, italic);
        let mut lines = self.wrap(text, max_width);
        lines.truncate(max_lines);
        Fitted {
            lines,
            line_height: min_size * line_ratio,
        }
    }

    /// `drawFittedText`: the lines centred on `center_y`, in the current font.
    fn draw_fitted(&mut self, fitted: &Fitted, x: f64, center_y: f64, align: Align, color: &str) {
        let block = fitted.lines.len() as f64 * fitted.line_height;
        let first = center_y - block / 2.0 + fitted.line_height / 2.0;
        for (index, line) in fitted.lines.iter().enumerate() {
            self.ops.push(Op::Text {
                text: line.clone(),
                x,
                y: first + index as f64 * fitted.line_height,
                font: self.font.clone(),
                align,
                color: Rgba::hex(color),
            });
        }
    }

    /// `drawSpacedText`: letter by letter with extra spacing, shrunk to at most 1200 wide.
    #[allow(clippy::too_many_arguments)]
    fn draw_spaced(
        &mut self,
        text: &str,
        (x, y): (f64, f64),
        family: &str,
        weight: u16,
        size: f64,
        spacing: f64,
        color: &str,
        align: Align,
    ) {
        let characters: Vec<String> = text.chars().map(String::from).collect();
        let gaps = characters.len().saturating_sub(1) as f64;
        let (mut size, mut spacing) = (size, spacing);
        self.set_font(size, family, weight, false);
        let mut widths: Vec<f64> = characters.iter().map(|c| self.width(c)).collect();
        let mut total = widths.iter().sum::<f64>() + spacing * gaps;
        if total > 1200.0 {
            let scale = 1200.0 / total;
            size = (size * scale).max(12.0);
            spacing *= scale;
            self.set_font(size, family, weight, false);
            widths = characters.iter().map(|c| self.width(c)).collect();
            total = widths.iter().sum::<f64>() + spacing * gaps;
        }
        let mut cursor = if align == Align::Center {
            x - total / 2.0
        } else {
            x
        };
        for (character, width) in characters.into_iter().zip(widths) {
            self.ops.push(Op::Text {
                text: character,
                x: cursor,
                y,
                font: self.font.clone(),
                align: Align::Left,
                color: Rgba::hex(color),
            });
            cursor += width + spacing;
        }
    }

    /// A centred title and subtitle (Classic and Minimal), grouped around y 1080.
    fn centred_titles(
        &mut self,
        text: &CoverText<'_>,
        palette: &CoverPalette,
        family: &str,
        title: ((f64, f64), (f64, f64), f64),
        subtitle: ((f64, f64), (f64, f64), f64),
        gap: f64,
    ) {
        let title = self.fit(text.title, family, 400, false, title.0, title.1, 5, title.2);
        let subtitle = (!text.subtitle.is_empty()).then(|| {
            self.fit(
                text.subtitle,
                family,
                400,
                true,
                subtitle.0,
                subtitle.1,
                3,
                subtitle.2,
            )
        });
        let gap = if subtitle.is_some() { gap } else { 0.0 };
        let title_block = title.lines.len() as f64 * title.line_height;
        let subtitle_block = subtitle
            .as_ref()
            .map_or(0.0, |s| s.lines.len() as f64 * s.line_height);
        let total = title_block + gap + subtitle_block;
        let group = 1080.0;
        self.draw_fitted(
            &title,
            800.0,
            group - total / 2.0 + title_block / 2.0,
            Align::Center,
            palette.ink,
        );
        if let Some(subtitle) = &subtitle {
            let center = group + total / 2.0 - subtitle_block / 2.0;
            self.draw_fitted(subtitle, 800.0, center, Align::Center, palette.muted);
        }
    }

    /// A left-aligned upper-case title with the subtitle under it (Modern and Bold).
    fn left_titles(
        &mut self,
        text: &CoverText<'_>,
        palette: &CoverPalette,
        family: &str,
        x: f64,
        title: ((f64, f64), (f64, f64), f64, f64),
        gap: f64,
    ) {
        let (bounds, sizes, ratio, center) = title;
        let fitted = self.fit(
            &text.title.to_uppercase(),
            family,
            800,
            false,
            bounds,
            sizes,
            5,
            ratio,
        );
        self.draw_fitted(&fitted, x, center, Align::Left, palette.ink);
        if !text.subtitle.is_empty() {
            let subtitle = self.fit(
                text.subtitle,
                family,
                400,
                false,
                (1080.0, 180.0),
                (50.0, 26.0),
                3,
                1.3,
            );
            let top = center + fitted.lines.len() as f64 * fitted.line_height / 2.0 + gap;
            let block = subtitle.lines.len() as f64 * subtitle.line_height;
            self.draw_fitted(&subtitle, x, top + block / 2.0, Align::Left, palette.muted);
        }
    }

    fn classic(&mut self, text: &CoverText<'_>, palette: &CoverPalette, family: &str) {
        self.background(palette);
        self.ops.push(Op::StrokeRect {
            x: 92.0,
            y: 92.0,
            width: f64::from(WIDTH) - 184.0,
            height: f64::from(HEIGHT) - 184.0,
            color: Rgba::with_alpha(palette.accent, 0.55),
            line: 2.0,
        });
        self.line(
            (650.0, 272.0),
            (950.0, 272.0),
            Rgba::with_alpha(palette.accent, 0.8),
            3.0,
        );
        self.ops.push(Op::Circle {
            x: 800.0,
            y: 272.0,
            radius: 7.0,
            color: Rgba::hex(palette.accent),
        });
        self.centred_titles(
            text,
            palette,
            family,
            ((1120.0, 760.0), (172.0, 80.0), 1.1),
            ((980.0, 190.0), (52.0, 28.0), 1.3),
            62.0,
        );
        if !text.author.is_empty() {
            self.draw_spaced(
                &text.author.to_uppercase(),
                (800.0, 2070.0),
                family,
                500,
                34.0,
                7.0,
                palette.accent,
                Align::Center,
            );
        }
        self.line(
            (700.0, 2150.0),
            (900.0, 2150.0),
            Rgba::with_alpha(palette.accent, 0.5),
            2.0,
        );
    }

    fn modern(&mut self, text: &CoverText<'_>, palette: &CoverPalette, family: &str) {
        self.background(palette);
        self.solid((118.0, 320.0, 18.0, 620.0), Rgba::hex(palette.accent));
        self.solid(
            (118.0, 1110.0, 1364.0, 4.0),
            Rgba::with_alpha(palette.accent, 0.24),
        );
        self.left_titles(
            text,
            palette,
            family,
            176.0,
            ((1230.0, 700.0), (188.0, 90.0), 1.04, 790.0),
            64.0,
        );
        if !text.author.is_empty() {
            self.draw_spaced(
                &text.author.to_uppercase(),
                (176.0, 2090.0),
                family,
                600,
                34.0,
                6.0,
                palette.accent,
                Align::Left,
            );
            self.line(
                (176.0, 2150.0),
                (660.0, 2150.0),
                Rgba::with_alpha(palette.accent, 0.65),
                3.0,
            );
        }
    }

    fn minimal(&mut self, text: &CoverText<'_>, palette: &CoverPalette, family: &str) {
        self.solid(
            (0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)),
            Rgba::hex(palette.background),
        );
        self.ops.push(Op::StrokeRect {
            x: 100.0,
            y: 100.0,
            width: f64::from(WIDTH) - 200.0,
            height: f64::from(HEIGHT) - 200.0,
            color: Rgba::with_alpha(palette.ink, 0.22),
            line: 2.0,
        });
        self.solid((770.0, 250.0, 60.0, 6.0), Rgba::hex(palette.accent));
        self.centred_titles(
            text,
            palette,
            family,
            ((1040.0, 680.0), (128.0, 64.0), 1.12),
            ((960.0, 180.0), (46.0, 26.0), 1.35),
            54.0,
        );
        if !text.author.is_empty() {
            self.draw_spaced(
                &text.author.to_uppercase(),
                (800.0, 2070.0),
                family,
                500,
                28.0,
                10.0,
                palette.accent,
                Align::Center,
            );
        }
        self.line(
            (760.0, 2140.0),
            (840.0, 2140.0),
            Rgba::with_alpha(palette.accent, 0.7),
            2.0,
        );
    }

    fn bold(&mut self, text: &CoverText<'_>, palette: &CoverPalette, family: &str) {
        self.background(palette);
        self.ops.push(Op::Circle {
            x: 1360.0,
            y: 480.0,
            radius: 520.0,
            color: Rgba::with_alpha(palette.accent, 0.16),
        });
        self.solid(
            (0.0, 0.0, f64::from(WIDTH), 18.0),
            Rgba::hex(palette.accent),
        );
        self.left_titles(
            text,
            palette,
            family,
            140.0,
            ((1270.0, 800.0), (210.0, 94.0), 1.02, 900.0),
            60.0,
        );
        if text.author.is_empty() {
            self.solid((140.0, 2140.0, 420.0, 8.0), Rgba::hex(palette.accent));
        } else {
            self.solid(
                (0.0, 2030.0, f64::from(WIDTH), 370.0),
                Rgba::hex(palette.accent),
            );
            self.draw_spaced(
                &text.author.to_uppercase(),
                (140.0, 2195.0),
                family,
                700,
                40.0,
                8.0,
                palette.background,
                Align::Left,
            );
        }
    }
}

/// `FittedText` (the font is the canvas's).
struct Fitted {
    lines: Vec<String>,
    line_height: f64,
}

/// What the cover shows.
pub struct CoverText<'a> {
    pub title: &'a str,
    pub subtitle: &'a str,
    pub author: &'a str,
}

/// `renderGeneratedCover` up to the pixels: what the design draws. `style_font` is the export
/// style's CSS font family, which Classic and Minimal use; Modern and Bold use Inter.
pub fn draw(
    text: &CoverText<'_>,
    design: &str,
    palette: &CoverPalette,
    style_font: &str,
    measure: &dyn Measure,
) -> Vec<Op> {
    let family = if matches!(design, "modern" | "bold") {
        SANS
    } else {
        style_font
    };
    let mut canvas = Canvas {
        ops: Vec::new(),
        // A new canvas's font.
        font: Font {
            family: "sans-serif".to_owned(),
            weight: 400,
            italic: false,
            size: 10.0,
        },
        measure,
    };
    match design {
        "modern" => canvas.modern(text, palette, family),
        "minimal" => canvas.minimal(text, palette, family),
        "bold" => canvas.bold(text, palette, family),
        _ => canvas.classic(text, palette, family),
    }
    canvas.ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_shade_like_prosecraft() {
        assert_eq!(shade("#141813", -0.16), "#111410");
        assert_eq!(shade("#dde4d6", 0.5), "#eef2eb");
        assert_eq!(
            Rgba::with_alpha("#c1a46b", 0.55).css(),
            "rgba(193, 164, 107, 0.55)"
        );
        assert_eq!(Rgba::hex("#C1A46B").css(), "#c1a46b");
    }
}
