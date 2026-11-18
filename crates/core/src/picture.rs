//! A picture for the text: an image file made into the embedded data URL ProseCraft stores
//! (`imageFileToDataUrl` and `insertImageFiles` in `src/routes/+page.svelte`, upstream
//! `2d1cf18`).
//!
//! ProseCraft draws the image on a canvas at most 2400 pixels on its longer side and keeps the
//! first of WebP at quality 0.82, 0.7 and 0.58 (then JPEG at the same qualities for a JPEG, then
//! PNG) whose base64 fits the embedded-image limit. The bytes can't match a browser's encoders;
//! the sizes, the order of attempts, the limits and the messages do.

use base64::Engine;

/// `MAX_IMAGE_FILE_BYTES`.
pub const MAX_FILE_BYTES: usize = 15_000_000;
/// `MAX_IMAGE_PIXELS`.
pub const MAX_PIXELS: u64 = 50_000_000;
/// `MAX_EMBEDDED_IMAGE_BYTES`, as base64: `Math.ceil(MAX / 3) * 4`.
pub const MAX_BASE64_LEN: usize = 1_500_000_usize.div_ceil(3) * 4;
/// The longest side a picture keeps.
pub const MAX_SIDE: u32 = 2400;

/// `IMAGE_MIME_BY_EXTENSION`: the files the picker and drops take.
pub const EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];

/// `IMAGE_ERROR_MESSAGE`.
pub const UNSUPPORTED: &str = "Use a PNG, JPG, WebP, GIF, BMP, or AVIF image.";

fn extension(name: &str) -> String {
    name.rsplit('.').next().unwrap_or_default().to_lowercase()
}

/// `isSupportedImage` for a file known by its name (a desktop file has no media type).
pub fn is_supported(name: &str) -> bool {
    name.contains('.') && EXTENSIONS.contains(&extension(name).as_str())
}

/// The picture's alternative text: the file name without its extension, at most 500 UTF-16
/// units (`file.name.replace(/\.[^.]+$/, "").slice(0, 500)`).
pub fn alt_text(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(dot) if dot + 1 < name.len() => &name[..dot],
        _ => name,
    };
    let mut units = 0;
    stem.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 500
        })
        .collect()
}

/// The `<img>` ProseCraft inserts for it.
pub fn image_html(src: &str, alt: &str) -> String {
    let attr = |v: &str| {
        v.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('\u{a0}', "&nbsp;")
    };
    format!(r#"<img src="{}" alt="{}">"#, attr(src), attr(alt))
}

/// Scales RGBA pixels down to `width` × `height` by averaging the source pixels each target
/// pixel covers (premultiplied, so transparent edges don't darken).
fn shrink(rgba: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    let (sw, sh) = (from.0 as usize, from.1 as usize);
    let (tw, th) = (to.0 as usize, to.1 as usize);
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        let y0 = ty * sh / th;
        let y1 = ((ty + 1) * sh).div_ceil(th).clamp(y0 + 1, sh);
        for tx in 0..tw {
            let x0 = tx * sw / tw;
            let x1 = ((tx + 1) * sw).div_ceil(tw).clamp(x0 + 1, sw);
            let mut sum = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = (y * sw + x) * 4;
                    let a = u64::from(rgba[p + 3]);
                    for c in 0..3 {
                        sum[c] += u64::from(rgba[p + c]) * a;
                    }
                    sum[3] += a;
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            let q = (ty * tw + tx) * 4;
            for c in 0..3 {
                if let Some(value) = (sum[c] + sum[3] / 2).checked_div(sum[3]) {
                    out[q + c] = value.min(255) as u8;
                }
            }
            out[q + 3] = ((sum[3] + count / 2) / count) as u8;
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

/// `imageFileToDataUrl`: the file as an embedded data URL, or ProseCraft's message for why it
/// can't be inserted.
pub fn prepare(name: &str, bytes: &[u8]) -> Result<String, String> {
    use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};
    if !is_supported(name) {
        return Err(UNSUPPORTED.to_owned());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Images must be 15 MB or smaller.".to_owned());
    }
    let image =
        craft_image_codecs::decode(bytes).map_err(|_| "Could not read that image.".to_owned())?;
    let (width, height) = image.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("That image has too many pixels to insert.".to_owned());
    }
    let scale = (f64::from(MAX_SIDE) / f64::from(width.max(height))).min(1.0);
    let size = (
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    );
    let mut rgba = image.to_rgba8();
    if size != (width, height) {
        rgba = shrink(&rgba, (width, height), size);
    }
    let canvas = Image::from_u8(size.0, size.1, ChannelLayout::Rgba, rgba.clone())
        .map_err(|_| "Could not prepare that image.".to_owned())?;
    let fits = |url: &str| url.len() - url.find(',').map_or(0, |c| c + 1) <= MAX_BASE64_LEN;
    for quality in [82, 70, 58] {
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
    if matches!(extension(name).as_str(), "jpg" | "jpeg") {
        // A canvas has no colour under transparent pixels; a JPEG of it is drawn on black.
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = u16::from(p[3]);
                [0, 1, 2].map(|c| ((u16::from(p[c]) * a + 127) / 255) as u8)
            })
            .collect();
        let opaque = Image::from_u8(size.0, size.1, ChannelLayout::Rgb, rgb)
            .map_err(|_| "Could not prepare that image.".to_owned())?;
        for quality in [82, 70, 58] {
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
    }
    let png = craft_image_codecs::encode(&canvas, Format::Png, &EncodeOptions::default())
        .map_err(|_| "Could not prepare that image.".to_owned())?;
    let url = data_url("image/png", &png);
    if fits(&url) {
        return Ok(url);
    }
    Err("That image is too large after compression. Try a smaller image.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|i| [(i % 251) as u8, (i % 13) as u8 * 19, 90, 255])
            .collect();
        let image = Image::from_u8(width, height, ChannelLayout::Rgba, pixels).expect("image");
        craft_image_codecs::encode(&image, Format::Png, &EncodeOptions::default()).expect("png")
    }

    #[test]
    fn a_picture_is_kept_as_webp_at_most_2400_pixels() {
        let url = prepare("Lighthouse.png", &png(3000, 1500)).expect("prepared");
        assert!(url.starts_with("data:image/webp;base64,"), "{}", &url[..40]);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&url["data:image/webp;base64,".len()..])
            .expect("base64");
        assert_eq!(
            craft_image_codecs::probe_size(&bytes).ok(),
            Some((2400, 1200))
        );
        assert!(crate::html::sanitize::is_embedded_image_src(&url));
        let small = prepare("dot.PNG", &png(3, 2)).expect("prepared");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&small[small.find(',').map_or(0, |c| c + 1)..])
            .expect("base64");
        assert_eq!(craft_image_codecs::probe_size(&bytes).ok(), Some((3, 2)));
    }

    #[test]
    fn transparency_survives() {
        use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image};
        let pixels: Vec<u8> = (0..64 * 64)
            .flat_map(|i| [200, 40, 40, if i % 64 < 32 { 255 } else { 0 }])
            .collect();
        let image = Image::from_u8(64, 64, ChannelLayout::Rgba, pixels).expect("image");
        let png = craft_image_codecs::encode(&image, Format::Png, &EncodeOptions::default())
            .expect("png");
        let url = prepare("half.png", &png).expect("prepared");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&url[url.find(',').map_or(0, |c| c + 1)..])
            .expect("base64");
        let back = craft_image_codecs::decode(&bytes)
            .expect("decoded")
            .to_rgba8();
        assert_eq!(back[3], 255);
        assert!(back[63 * 4 + 3] < 16, "alpha {}", back[63 * 4 + 3]);
    }

    #[test]
    fn refuses_as_prosecraft_does() {
        assert_eq!(prepare("notes.txt", b"x"), Err(UNSUPPORTED.to_owned()));
        assert_eq!(prepare("png", b"x"), Err(UNSUPPORTED.to_owned()));
        assert_eq!(
            prepare("broken.png", b"not a png"),
            Err("Could not read that image.".to_owned())
        );
        assert_eq!(
            prepare("huge.jpg", &vec![0; MAX_FILE_BYTES + 1]),
            Err("Images must be 15 MB or smaller.".to_owned())
        );
    }

    #[test]
    fn names_and_markup() {
        assert_eq!(
            alt_text("Map of the coast.final.png"),
            "Map of the coast.final"
        );
        assert_eq!(alt_text("noext"), "noext");
        assert_eq!(
            alt_text(&format!("{}.png", "é".repeat(600)))
                .chars()
                .count(),
            500
        );
        assert_eq!(
            image_html("data:image/png;base64,AA==", r#"A "quoted" & odd"#),
            r#"<img src="data:image/png;base64,AA==" alt="A &quot;quoted&quot; &amp; odd">"#
        );
        assert!(is_supported("x.JPEG") && !is_supported("x.tiff"));
    }

    #[test]
    fn shrinking_averages_and_keeps_transparent_edges_light() {
        // Two white opaque pixels and two transparent black ones become one white pixel at half
        // alpha, not grey.
        let px = [
            255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
        ];
        assert_eq!(shrink(&px, (2, 2), (1, 1)), [255, 255, 255, 128]);
    }
}
