//! Book covers (`src/lib/publish/cover.ts` and `normalizeCover` in `types.ts`): the cover
//! options a project stores, an uploaded image made into a 2:3 cover, and the cover an export
//! draws.
//!
//! ProseCraft crops and scales an uploaded image on a canvas to 1600 × 2400 and keeps the first
//! of WebP at quality 0.86, 0.74 and 0.62, then JPEG at 0.9, 0.8 and 0.7, then PNG, whose base64
//! fits the embedded-image limit. The bytes can't match a browser's encoders; the size, the
//! order of attempts, the limits and the messages do.

use base64::Engine;

use crate::js::{JsObject, JsValue};

/// `COVER_WIDTH` × `COVER_HEIGHT`.
pub const WIDTH: u32 = 1600;
pub const HEIGHT: u32 = 2400;
/// `MAX_COVER_FILE_BYTES`.
pub const MAX_FILE_BYTES: usize = 15_000_000;
/// `MAX_COVER_PIXELS`.
pub const MAX_PIXELS: u64 = 50_000_000;
/// The files a cover can be made from (`COVER_ACCEPT`).
pub const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// `COVER_MODE_IDS`.
pub const MODES: [&str; 3] = ["none", "image", "generated"];

/// `COVER_DESIGNS`: id, label, detail.
pub const DESIGNS: [(&str, &str, &str); 4] = [
    ("classic", "Classic", "Centered serif and fine rules"),
    ("modern", "Modern", "Bold sans title with an accent bar"),
    ("minimal", "Minimal", "Quiet type inside a hairline frame"),
    ("bold", "Bold", "Oversized type and a color block"),
];

/// A generated cover's colours (`CoverPaletteOption`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverPalette {
    pub id: &'static str,
    pub label: &'static str,
    pub background: &'static str,
    pub ink: &'static str,
    pub accent: &'static str,
    pub muted: &'static str,
}

/// `COVER_PALETTES`.
pub const PALETTES: [CoverPalette; 5] = [
    CoverPalette {
        id: "ink",
        label: "Ink",
        background: "#141813",
        ink: "#f1eadb",
        accent: "#c1a46b",
        muted: "#b8b1a3",
    },
    CoverPalette {
        id: "sage",
        label: "Sage",
        background: "#dde4d6",
        ink: "#23352a",
        accent: "#6f8a6b",
        muted: "#5c6b5b",
    },
    CoverPalette {
        id: "clay",
        label: "Clay",
        background: "#a84e38",
        ink: "#fff1e3",
        accent: "#f0c19d",
        muted: "#f2d7c4",
    },
    CoverPalette {
        id: "ocean",
        label: "Ocean",
        background: "#173545",
        ink: "#eef6f5",
        accent: "#79c2c6",
        muted: "#a8c3c7",
    },
    CoverPalette {
        id: "plum",
        label: "Plum",
        background: "#3b203a",
        ink: "#f7e9ef",
        accent: "#d89bb3",
        muted: "#c9afbd",
    },
];

/// `paletteFor`: an unknown id is Ink.
pub fn palette_for(id: &str) -> &'static CoverPalette {
    PALETTES.iter().find(|p| p.id == id).unwrap_or(&PALETTES[0])
}

/// `designFor`: an unknown id is Classic.
pub fn design_for(id: &str) -> (&'static str, &'static str, &'static str) {
    DESIGNS
        .iter()
        .find(|d| d.0 == id)
        .copied()
        .unwrap_or(DESIGNS[0])
}

/// `CoverOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverOptions {
    pub mode: &'static str,
    /// The uploaded cover as an embedded data URL; empty unless `mode` is "image".
    pub image: String,
    pub design: &'static str,
    pub palette: &'static str,
}

impl Default for CoverOptions {
    fn default() -> Self {
        Self {
            mode: "none",
            image: String::new(),
            design: "classic",
            palette: "ink",
        }
    }
}

fn one_of<const N: usize>(value: Option<&JsValue>, ids: [&'static str; N]) -> Option<&'static str> {
    let value = value?.as_js_string()?.to_string_lossy();
    ids.into_iter().find(|id| *id == value)
}

impl CoverOptions {
    /// `normalizeCover`: unknown values fall back to the defaults, and an image cover without a
    /// usable image is no cover.
    pub fn read(value: Option<&JsValue>) -> Self {
        let defaults = Self::default();
        let Some(value) = value.filter(|v| v.as_object().is_some()) else {
            return defaults;
        };
        let mode = one_of(value.get("mode"), MODES).unwrap_or(defaults.mode);
        let design = one_of(value.get("design"), DESIGNS.map(|d| d.0)).unwrap_or(defaults.design);
        let palette =
            one_of(value.get("palette"), PALETTES.map(|p| p.id)).unwrap_or(defaults.palette);
        let image = value
            .get("image")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .filter(|s| crate::html::sanitize::is_embedded_image_src(s))
            .unwrap_or_default();
        Self {
            mode: if mode == "image" && image.is_empty() {
                "none"
            } else {
                mode
            },
            image: if mode == "image" {
                image
            } else {
                String::new()
            },
            design,
            palette,
        }
    }

    /// As ProseCraft stores it.
    pub fn to_value(&self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("mode", JsValue::str(self.mode));
        o.insert("image", JsValue::str(&self.image));
        o.insert("design", JsValue::str(self.design));
        o.insert("palette", JsValue::str(self.palette));
        JsValue::Object(o)
    }
}

/// `CompiledCover`: the cover an export draws.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledCover {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    /// "image/png", "image/jpeg" or "image/webp".
    pub media_type: &'static str,
    /// `#rrggbb`, drawn behind the cover where the page's shape differs from it.
    pub background: String,
}

/// `mediaTypeFromDataUrl`.
pub fn media_type(src: &str) -> &'static str {
    ["image/png", "image/jpeg", "image/webp"]
        .into_iter()
        .find(|t| {
            src.strip_prefix("data:")
                .and_then(|s| s.strip_prefix(t))
                .is_some_and(|s| s.starts_with(";base64,"))
        })
        .unwrap_or("image/png")
}

fn decode_data_url(src: &str) -> Option<Vec<u8>> {
    let (_, data) = src.split_once(";base64,")?;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// `averageImageColor`: the image drawn into one pixel, as `#rrggbb` (transparent pixels count
/// for nothing, and a fully transparent image is black).
pub fn average_color(rgba: &[u8]) -> String {
    let mut sum = [0u64; 4];
    for p in rgba.as_chunks::<4>().0 {
        let a = u64::from(p[3]);
        for c in 0..3 {
            sum[c] += u64::from(p[c]) * a;
        }
        sum[3] += a;
    }
    let channel = |c: usize| {
        (sum[c] + sum[3] / 2)
            .checked_div(sum[3])
            .unwrap_or(0)
            .min(255)
    };
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

/// `resolveCoverImage` for an uploaded cover: its size, type and average colour. `None` for no
/// cover, a generated one (drawn by the app, see `cover_art`), or an image that can't be read.
pub fn resolve_image(options: &CoverOptions) -> Option<CompiledCover> {
    if options.mode != "image" || options.image.is_empty() {
        return None;
    }
    let bytes = decode_data_url(&options.image)?;
    let image = craft_image_codecs::decode(&bytes).ok()?;
    let (width, height) = image.dimensions();
    Some(CompiledCover {
        data_url: options.image.clone(),
        width,
        height,
        media_type: media_type(&options.image),
        background: average_color(&image.to_rgba8()),
    })
}

/// `isSupportedCoverFile` for a file known by its name.
pub fn is_supported(name: &str) -> bool {
    let extension = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    name.contains('.') && EXTENSIONS.contains(&extension.as_str())
}

/// One sample of the source at (`x`, `y`) in pixels, between pixel centres (premultiplied).
fn bilinear(rgba: &[u8], (w, h): (usize, usize), x: f64, y: f64) -> [f64; 4] {
    let x = (x - 0.5).clamp(0.0, (w - 1) as f64);
    let y = (y - 0.5).clamp(0.0, (h - 1) as f64);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let px = |x: usize, y: usize| {
        let p = (y * w + x) * 4;
        let a = f64::from(rgba[p + 3]) / 255.0;
        [
            f64::from(rgba[p]) * a,
            f64::from(rgba[p + 1]) * a,
            f64::from(rgba[p + 2]) * a,
            f64::from(rgba[p + 3]),
        ]
    };
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    std::array::from_fn(|i| {
        (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
    })
}

/// `drawCoverCrop`: the middle of the image in the cover's 2:3 shape, scaled to 1600 × 2400 on
/// white. Shrinking averages every source pixel under a cover pixel; enlarging is bilinear.
pub fn crop(rgba: &[u8], (w, h): (u32, u32)) -> Vec<u8> {
    let (sw, sh) = (f64::from(w), f64::from(h));
    let target = f64::from(WIDTH) / f64::from(HEIGHT);
    let (mut cw, mut ch, mut cx, mut cy) = (sw, sh, 0.0, 0.0);
    if sw / sh > target {
        cw = sh * target;
        cx = (sw - cw) / 2.0;
    } else {
        ch = sw / target;
        cy = (sh - ch) / 2.0;
    }
    let (tw, th) = (WIDTH as usize, HEIGHT as usize);
    let (step_x, step_y) = (cw / tw as f64, ch / th as f64);
    // Samples per cover pixel on each axis: enough to cover the source pixels it spans.
    let (nx, ny) = (
        step_x.ceil().max(1.0) as usize,
        step_y.ceil().max(1.0) as usize,
    );
    let size = (w as usize, h as usize);
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        for tx in 0..tw {
            let mut sum = [0.0; 4];
            for j in 0..ny {
                for i in 0..nx {
                    let x = cx + (tx as f64 + (i as f64 + 0.5) / nx as f64) * step_x;
                    let y = cy + (ty as f64 + (j as f64 + 0.5) / ny as f64) * step_y;
                    let s = bilinear(rgba, size, x, y);
                    for k in 0..4 {
                        sum[k] += s[k];
                    }
                }
            }
            let n = (nx * ny) as f64;
            let alpha = sum[3] / n / 255.0;
            let q = (ty * tw + tx) * 4;
            // Over white.
            for k in 0..3 {
                out[q + k] = (sum[k] / n + 255.0 * (1.0 - alpha))
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            out[q + 3] = 255;
        }
    }
    out
}

fn data_url(media: &str, bytes: &[u8]) -> String {
    format!(
        "data:{media};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// `encodedImageFits`.
fn fits(url: &str) -> bool {
    url.len() - url.find(',').map_or(0, |c| c + 1) <= crate::picture::MAX_BASE64_LEN
}

/// `encodeCoverCanvas`: the cover's pixels as the first data URL that fits.
pub fn encode(rgba: Vec<u8>) -> Result<String, String> {
    use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};
    let failed = || "Could not prepare that cover image.".to_owned();
    let canvas =
        Image::from_u8(WIDTH, HEIGHT, ChannelLayout::Rgba, rgba.clone()).map_err(|_| failed())?;
    for quality in [86, 74, 62] {
        let options = EncodeOptions {
            webp_lossless: false,
            webp_quality: quality,
            ..EncodeOptions::default()
        };
        if let Ok(webp) = craft_image_codecs::encode(&canvas, Format::WebP, &options) {
            let url = data_url("image/webp", &webp);
            if fits(&url) {
                return Ok(url);
            }
        }
    }
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let opaque = Image::from_u8(WIDTH, HEIGHT, ChannelLayout::Rgb, rgb).map_err(|_| failed())?;
    for quality in [90, 80, 70] {
        let options = EncodeOptions {
            jpeg_quality: quality,
            ..EncodeOptions::default()
        };
        if let Ok(jpeg) = craft_image_codecs::encode(&opaque, Format::Jpeg, &options) {
            let url = data_url("image/jpeg", &jpeg);
            if fits(&url) {
                return Ok(url);
            }
        }
    }
    let png = craft_image_codecs::encode(&canvas, Format::Png, &EncodeOptions::default())
        .map_err(|_| failed())?;
    let url = data_url("image/png", &png);
    if fits(&url) {
        return Ok(url);
    }
    Err("That cover image is too large after compression. Try a smaller image.".to_owned())
}

fn check_pixels(w: u32, h: u32) -> Result<(), String> {
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("That image has too many pixels to use as a cover.".to_owned());
    }
    Ok(())
}

/// `imageFileToCoverDataUrl`: an image file made into the stored cover, or ProseCraft's
/// message for why it can't be.
pub fn prepare_image(name: &str, bytes: &[u8]) -> Result<String, String> {
    if !is_supported(name) {
        return Err("Use a PNG, JPG, or WebP image.".to_owned());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Cover images must be 15 MB or smaller.".to_owned());
    }
    let undecodable = || "Could not decode that image.".to_owned();
    // The pixel count is checked from the header, before the image is decoded.
    let (w, h) = craft_image_codecs::probe_size(bytes).map_err(|_| undecodable())?;
    check_pixels(w, h)?;
    let image = craft_image_codecs::decode(bytes).map_err(|_| undecodable())?;
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return Err(undecodable());
    }
    encode(crop(&image.to_rgba8(), (w, h)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};

    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend(pixel(x, y));
            }
        }
        let image = Image::from_u8(width, height, ChannelLayout::Rgba, rgba).expect("image");
        craft_image_codecs::encode(&image, Format::Png, &EncodeOptions::default()).expect("png")
    }

    #[test]
    fn options_read_like_normalize_cover() {
        let read = |text: &str| CoverOptions::read(Some(&crate::js::parse(text).expect("json")));
        assert_eq!(CoverOptions::read(None), CoverOptions::default());
        assert_eq!(read("[]"), CoverOptions::default());
        let picked = read(
            r#"{"mode":"generated","design":"bold","palette":"plum","image":"data:image/png;base64,AAAA"}"#,
        );
        assert_eq!(
            (
                picked.mode,
                picked.design,
                picked.palette,
                picked.image.as_str()
            ),
            ("generated", "bold", "plum", "")
        );
        // An image cover without an image is none; unknown ids fall back.
        let none =
            read(r#"{"mode":"image","image":"https://x/y.png","design":"wavy","palette":"neon"}"#);
        assert_eq!(
            (none.mode, none.design, none.palette),
            ("none", "classic", "ink")
        );
        let image = read(r#"{"mode":"image","image":"data:image/webp;base64,AAAA"}"#);
        assert_eq!(
            (image.mode, image.image.as_str()),
            ("image", "data:image/webp;base64,AAAA")
        );
        assert_eq!(CoverOptions::read(Some(&image.to_value())), image);
    }

    #[test]
    fn an_image_is_cropped_to_the_middle_of_a_2_by_3_cover() {
        // Wide: red on the left and right quarters, blue in the middle half.
        let wide = png(400, 200, |x, _| {
            if (100..300).contains(&x) {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            }
        });
        let url = prepare_image("wide.png", &wide).expect("a cover");
        assert!(url.starts_with("data:image/webp;base64,"), "{}", &url[..30]);
        let image =
            craft_image_codecs::decode(&decode_data_url(&url).expect("bytes")).expect("decoded");
        assert_eq!(image.dimensions(), (WIDTH, HEIGHT));
        // The middle 133 of 400 columns are kept: all blue.
        let rgba = image.to_rgba8();
        for x in [10, 800, 1590] {
            let p = &rgba[(1200 * WIDTH as usize + x) * 4..][..3];
            assert!(p[2] > 200 && p[0] < 40, "{x}: {p:?}");
        }
        // Transparent pixels come out white.
        let clear = png(20, 30, |_, _| [0, 0, 0, 0]);
        let rgba = crop(
            &craft_image_codecs::decode(&clear)
                .expect("decoded")
                .to_rgba8(),
            (20, 30),
        );
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn files_are_refused_with_prosecrafts_messages() {
        assert_eq!(
            prepare_image("cover.gif", b"GIF89a").unwrap_err(),
            "Use a PNG, JPG, or WebP image."
        );
        assert_eq!(
            prepare_image("cover", b"x").unwrap_err(),
            "Use a PNG, JPG, or WebP image."
        );
        assert_eq!(
            prepare_image("cover.png", b"not a png").unwrap_err(),
            "Could not decode that image."
        );
        let big = vec![0u8; MAX_FILE_BYTES + 1];
        assert_eq!(
            prepare_image("cover.jpg", &big).unwrap_err(),
            "Cover images must be 15 MB or smaller."
        );
        // 50 million pixels is the most a cover can come from.
        assert_eq!(check_pixels(10_000, 5_000), Ok(()));
        assert_eq!(
            check_pixels(10_001, 5_000).unwrap_err(),
            "That image has too many pixels to use as a cover."
        );
    }

    #[test]
    fn an_uploaded_cover_resolves_to_its_size_and_average_colour() {
        let red_blue = png(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let url = data_url("image/png", &red_blue);
        let options = CoverOptions {
            mode: "image",
            image: url.clone(),
            ..CoverOptions::default()
        };
        let cover = resolve_image(&options).expect("cover");
        assert_eq!(
            (cover.width, cover.height, cover.media_type),
            (2, 1, "image/png")
        );
        assert_eq!(cover.background, "#800080");
        assert_eq!(resolve_image(&CoverOptions::default()), None);
        assert_eq!(media_type("data:image/jpeg;base64,AA"), "image/jpeg");
        assert_eq!(media_type("data:image/gif;base64,AA"), "image/png");
    }
}
