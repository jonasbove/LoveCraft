//! An embedded image's layout (ProseCraft `src/lib/image-layout.ts`): its size in the text
//! column, alignment and text wrap, kept as validated `data-` attributes.

use super::Element;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLayout {
    pub size: &'static str,
    /// Percent of the column, for the custom size.
    pub width: Option<u32>,
    pub align: &'static str,
    pub wrap: &'static str,
}

pub const DEFAULT: ImageLayout = ImageLayout {
    size: "large",
    width: None,
    align: "center",
    wrap: "block",
};

const SIZES: [&str; 5] = ["small", "medium", "large", "full", "custom"];
const ALIGNS: [&str; 3] = ["left", "center", "right"];
const WRAPS: [&str; 3] = ["block", "wrap-left", "wrap-right"];

fn pick(value: Option<&str>, allowed: &[&'static str]) -> Option<&'static str> {
    value.and_then(|v| allowed.iter().find(|a| **a == v).copied())
}

/// `asWidth` for an attribute: 1–3 digits with up to 2 decimals, rounded and kept in 10–100.
fn width(value: Option<&str>) -> Option<u32> {
    let value = value?;
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let digits = |s: &str, min: usize, max: usize| {
        (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(whole, 1, 3) || (value.contains('.') && !digits(fraction, 1, 2)) {
        return None;
    }
    let n: f64 = value.parse().ok()?;
    // Math.round: halves round up.
    let rounded = (n + 0.5).floor();
    Some(rounded.clamp(10.0, 100.0) as u32)
}

/// `readImageLayout`: an image's layout, with defaults for anything unknown. A custom size
/// without a usable width reads as the default size.
pub fn read(image: &Element) -> ImageLayout {
    let align = pick(image.attr("data-align"), &ALIGNS).unwrap_or(DEFAULT.align);
    let wrap = pick(image.attr("data-wrap"), &WRAPS).unwrap_or(DEFAULT.wrap);
    let size = pick(image.attr("data-size"), &SIZES).unwrap_or(DEFAULT.size);
    if size == "custom" {
        return match width(image.attr("data-width")) {
            Some(width) => ImageLayout {
                size,
                width: Some(width),
                align,
                wrap,
            },
            None => ImageLayout {
                size: DEFAULT.size,
                width: None,
                align,
                wrap,
            },
        };
    }
    ImageLayout {
        size,
        width: None,
        align,
        wrap,
    }
}

/// The attributes `applyImageLayout` writes (only those that differ from the defaults), in
/// its order.
pub fn attributes(layout: &ImageLayout) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if layout.size != DEFAULT.size {
        out.push(("data-size".to_owned(), layout.size.to_owned()));
    }
    if layout.size == "custom" {
        out.push((
            "data-width".to_owned(),
            layout.width.unwrap_or(100).to_string(),
        ));
    }
    if layout.align != DEFAULT.align {
        out.push(("data-align".to_owned(), layout.align.to_owned()));
    }
    if layout.wrap != DEFAULT.wrap {
        out.push(("data-wrap".to_owned(), layout.wrap.to_owned()));
    }
    out
}
