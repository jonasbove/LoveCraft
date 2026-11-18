//! `decodeEmbeddedImage`: an image embedded in the text as a data URL, ready to package
//! (ProseCraft `src/lib/publish/embedded-image.ts`, upstream `2d1cf18`).
//!
//! PNG and JPEG are kept as they are. WebP is re-encoded as PNG, as ProseCraft does, so
//! readers and word processors without WebP support still show it. ProseCraft learns the size
//! by loading the image in the browser; here it comes from the file's header (with the EXIF
//! orientation applied, as a browser's natural size has it), or from the decoded WebP.

use base64::Engine;

/// An embedded image's bytes, file extension, media type and natural size in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
    pub media_type: &'static str,
    pub width: u32,
    pub height: u32,
}

/// The image in a `data:image/{png,jpeg,webp};base64,` URL, or why it can't be used.
pub fn decode_embedded_image(src: &str) -> Result<EmbeddedImage, String> {
    let (kind, data) = ["png", "jpeg", "webp"]
        .iter()
        .find_map(|k| {
            src.strip_prefix(&format!("data:image/{k};base64,"))
                .map(|d| (*k, d))
        })
        .ok_or_else(|| "not an embedded PNG, JPEG or WebP".to_owned())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())?;
    match kind {
        "webp" => {
            let image = craft_image_codecs::decode(&bytes).map_err(|e| e.to_string())?;
            let (width, height) = image.dimensions();
            let png = craft_image_codecs::encode(
                &image,
                craft_image_codecs::Format::Png,
                &craft_image_codecs::EncodeOptions::default(),
            )
            .map_err(|e| e.to_string())?;
            Ok(EmbeddedImage {
                bytes: png,
                extension: "png",
                media_type: "image/png",
                width,
                height,
            })
        }
        _ => {
            let (width, height) =
                craft_image_codecs::probe_size(&bytes).map_err(|e| e.to_string())?;
            let (extension, media_type) = if kind == "jpeg" {
                ("jpg", "image/jpeg")
            } else {
                ("png", "image/png")
            };
            Ok(EmbeddedImage {
                bytes,
                extension,
                media_type,
                width,
                height,
            })
        }
    }
}
