//! Writing a laid-out book ([`super::pdf::Layout`]) as a PDF file, on craft-pdf's object layer:
//! the standard fonts by name with WinAnsiEncoding, as jsPDF writes them (not embedded),
//! compressed page content, JPEG images as they are, PNG and WebP as RGB with an alpha mask.

use std::collections::HashMap;

use base64::Engine;
use craft_pdf::cos::{Dict, Document, Object, PdfString, SaveOptions, Stream, write_full};

use super::pdf::{ImageSizes, Layout, Op, Rgb, layout_pdf};
use super::pdf_text::{Face, Family, win_ansi};
use super::{CompiledBook, Export, PublishOptions, slugify_filename};

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("an image couldn't be read: {0}")]
    Image(String),
    #[error("the PDF couldn't be written: {0}")]
    Write(String),
}

/// A `data:` URL's kind and bytes.
fn data_url(src: &str) -> Option<(&'static str, Vec<u8>)> {
    let (kind, data) = ["png", "jpeg", "webp"].iter().find_map(|k| {
        src.strip_prefix(&format!("data:image/{k};base64,"))
            .map(|d| (*k, d))
    })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some((kind, bytes))
}

/// Image sizes read from the images' headers, as `getImageProperties` gives them.
#[derive(Default)]
pub struct DataUrlSizes {
    cache: std::cell::RefCell<HashMap<String, Option<(f64, f64)>>>,
}

impl ImageSizes for DataUrlSizes {
    fn size(&self, src: &str) -> Option<(f64, f64)> {
        if let Some(size) = self.cache.borrow().get(src) {
            return *size;
        }
        let size = data_url(src)
            .and_then(|(_, bytes)| craft_image_codecs::probe_size(&bytes).ok())
            .map(|(w, h)| (f64::from(w), f64::from(h)));
        self.cache.borrow_mut().insert(src.to_owned(), size);
        size
    }
}

/// A number as a content stream writes it: up to four decimals, no trailing zeros.
fn num(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A colour as jsPDF writes it: fill with three decimals, stroke with two.
fn color(c: Rgb, decimals: usize) -> String {
    let part = |v: u8| format!("{:.*}", decimals, f64::from(v) / 255.0);
    format!("{} {} {}", part(c[0]), part(c[1]), part(c[2]))
}

/// The text as a literal string in WinAnsiEncoding (the layout already replaced what it can't
/// show).
fn win_ansi_literal(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in text.chars() {
        let byte = win_ansi(c).unwrap_or(b'?');
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out.push(b')');
    out
}

fn face_index(face: Face) -> usize {
    let family = match face.family {
        Family::Times => 0,
        Family::Helvetica => 4,
        Family::Courier => 8,
    };
    family + usize::from(face.bold) + 2 * usize::from(face.italic)
}

fn key(name: &str) -> Vec<u8> {
    name.as_bytes().to_vec()
}

/// An image XObject for an embedded image.
fn image_object(doc: &mut Document, src: &str) -> Result<craft_pdf::cos::ObjRef, PdfError> {
    let (kind, bytes) =
        data_url(src).ok_or_else(|| PdfError::Image("not an embedded image".into()))?;
    let image = craft_image_codecs::decode(&bytes).map_err(|e| PdfError::Image(e.to_string()))?;
    let (width, height) = image.dimensions();
    let mut dict = Dict::new();
    dict.set(key("Type"), Object::name("XObject"));
    dict.set(key("Subtype"), Object::name("Image"));
    dict.set(key("Width"), Object::Int(i64::from(width)));
    dict.set(key("Height"), Object::Int(i64::from(height)));
    dict.set(key("BitsPerComponent"), Object::Int(8));
    use craft_image_codecs::ChannelLayout;
    let layout = image.layout();
    if kind == "jpeg" && matches!(layout, ChannelLayout::Rgb | ChannelLayout::Gray) {
        let space = if layout == ChannelLayout::Gray {
            "DeviceGray"
        } else {
            "DeviceRGB"
        };
        dict.set(key("ColorSpace"), Object::name(space));
        dict.set(key("Filter"), Object::name("DCTDecode"));
        return Ok(doc.add(Object::Stream(Stream::from_raw(dict, bytes))));
    }
    let rgba = image.to_rgba8();
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(rgba.len() / 4);
    for px in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3]);
    }
    dict.set(key("ColorSpace"), Object::name("DeviceRGB"));
    if alpha.iter().any(|a| *a != 255) {
        let mut mask = Dict::new();
        mask.set(key("Type"), Object::name("XObject"));
        mask.set(key("Subtype"), Object::name("Image"));
        mask.set(key("Width"), Object::Int(i64::from(width)));
        mask.set(key("Height"), Object::Int(i64::from(height)));
        mask.set(key("BitsPerComponent"), Object::Int(8));
        mask.set(key("ColorSpace"), Object::name("DeviceGray"));
        let mask = doc.add(Object::Stream(Stream::flate(mask, &alpha)));
        dict.set(key("SMask"), Object::Ref(mask));
    }
    Ok(doc.add(Object::Stream(Stream::flate(dict, &rgb))))
}

/// The layout as a PDF file.
pub fn write_pdf(layout: &Layout) -> Result<Vec<u8>, PdfError> {
    let mut doc = Document::new_empty();
    let pages_ref = doc
        .root()
        .and_then(|root| doc.get(root).as_dict().and_then(|d| d.reference(b"Pages")))
        .ok_or_else(|| PdfError::Write("no page tree".into()))?;
    let mut fonts: HashMap<usize, craft_pdf::cos::ObjRef> = HashMap::new();
    let mut images: HashMap<String, (usize, craft_pdf::cos::ObjRef)> = HashMap::new();
    let mut kids = Vec::new();
    for page in &layout.pages {
        let mut content = String::new();
        let mut page_fonts: Vec<usize> = Vec::new();
        let mut page_images: Vec<usize> = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        for op in &page.ops {
            match op {
                Op::Text {
                    x,
                    y,
                    face,
                    size,
                    color: c,
                    text,
                } => {
                    let index = face_index(*face);
                    if !page_fonts.contains(&index) {
                        page_fonts.push(index);
                    }
                    content.push_str(&format!(
                        "BT\n/F{} {} Tf\n{} rg\n{} {} Td\n",
                        index + 1,
                        num(*size),
                        color(*c, 3),
                        num(*x),
                        num(layout.height - y)
                    ));
                    body.extend_from_slice(content.as_bytes());
                    content.clear();
                    body.extend_from_slice(&win_ansi_literal(text));
                    body.extend_from_slice(b" Tj\nET\n");
                }
                Op::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: c,
                    width,
                } => {
                    body.extend_from_slice(
                        format!(
                            "{} RG\n{} w\n{} {} m\n{} {} l\nS\n",
                            color(*c, 2),
                            num(*width),
                            num(*x1),
                            num(layout.height - y1),
                            num(*x2),
                            num(layout.height - y2)
                        )
                        .as_bytes(),
                    );
                }
                Op::Rect {
                    x,
                    y,
                    width,
                    height,
                    color: c,
                } => {
                    body.extend_from_slice(
                        format!(
                            "{} rg\n{} {} {} {} re\nf\n",
                            // `setFillColor` writes two decimals.
                            color(*c, 2),
                            num(*x),
                            num(layout.height - y),
                            num(*width),
                            num(-height)
                        )
                        .as_bytes(),
                    );
                }
                Op::Image {
                    src,
                    x,
                    y,
                    width,
                    height,
                } => {
                    let number = match images.get(src) {
                        Some((n, _)) => *n,
                        None => {
                            let r = image_object(&mut doc, src)?;
                            let n = images.len() + 1;
                            images.insert(src.clone(), (n, r));
                            n
                        }
                    };
                    if !page_images.contains(&number) {
                        page_images.push(number);
                    }
                    body.extend_from_slice(
                        format!(
                            "q\n{} 0 0 {} {} {} cm\n/Im{number} Do\nQ\n",
                            num(*width),
                            num(*height),
                            num(*x),
                            num(layout.height - y - height)
                        )
                        .as_bytes(),
                    );
                }
            }
        }
        let mut font_dict = Dict::new();
        for index in &page_fonts {
            let font = match fonts.get(index) {
                Some(r) => *r,
                None => {
                    let face = super::pdf_text::FACES_IN_ORDER[*index];
                    let mut f = Dict::new();
                    f.set(key("Type"), Object::name("Font"));
                    f.set(key("Subtype"), Object::name("Type1"));
                    f.set(key("BaseFont"), Object::name(face.postscript_name()));
                    f.set(key("Encoding"), Object::name("WinAnsiEncoding"));
                    let r = doc.add(Object::Dict(f));
                    fonts.insert(*index, r);
                    r
                }
            };
            font_dict.set(key(&format!("F{}", index + 1)), Object::Ref(font));
        }
        let mut resources = Dict::new();
        resources.set(key("Font"), Object::Dict(font_dict));
        if !page_images.is_empty() {
            let mut xobjects = Dict::new();
            for (src, (number, r)) in &images {
                let _ = src;
                if page_images.contains(number) {
                    xobjects.set(key(&format!("Im{number}")), Object::Ref(*r));
                }
            }
            resources.set(key("XObject"), Object::Dict(xobjects));
        }
        let contents = doc.add(Object::Stream(Stream::flate(Dict::new(), &body)));
        let mut page_dict = Dict::new();
        page_dict.set(key("Type"), Object::name("Page"));
        page_dict.set(key("Parent"), Object::Ref(pages_ref));
        page_dict.set(
            key("MediaBox"),
            Object::Array(vec![
                Object::Int(0),
                Object::Int(0),
                Object::Real(layout.width),
                Object::Real(layout.height),
            ]),
        );
        page_dict.set(key("Resources"), Object::Dict(resources));
        page_dict.set(key("Contents"), Object::Ref(contents));
        kids.push(Object::Ref(doc.add(Object::Dict(page_dict))));
    }
    let mut pages = Dict::new();
    pages.set(key("Type"), Object::name("Pages"));
    pages.set(key("Count"), Object::Int(kids.len() as i64));
    pages.set(key("Kids"), Object::Array(kids));
    doc.set(pages_ref, Object::Dict(pages));
    let mut info = Dict::new();
    info.set(key("Title"), Object::String(PdfString::text(&layout.title)));
    if !layout.author.is_empty() {
        info.set(
            key("Author"),
            Object::String(PdfString::text(&layout.author)),
        );
    }
    if !layout.subtitle.is_empty() {
        info.set(
            key("Subject"),
            Object::String(PdfString::text(&layout.subtitle)),
        );
    }
    info.set(
        key("Creator"),
        Object::String(PdfString::text("prosecraft-rs")),
    );
    let info = doc.add(Object::Dict(info));
    doc.trailer_mut().set(key("Info"), Object::Ref(info));
    write_full(&doc, &SaveOptions::default()).map_err(|e| PdfError::Write(e.to_string()))
}

/// `exportPdf`: the book laid out and written, and how many characters the standard fonts
/// couldn't show (drawn as "?").
pub fn export_pdf(
    book: &CompiledBook,
    options: &PublishOptions,
) -> Result<(Export, usize), PdfError> {
    let layout = layout_pdf(book, options, &DataUrlSizes::default());
    let bytes = write_pdf(&layout)?;
    Ok((
        Export {
            file_name: format!("{}.pdf", slugify_filename(&book.title)),
            bytes,
        },
        layout.unshown,
    ))
}
