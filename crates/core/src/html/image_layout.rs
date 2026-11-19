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

/// `align` as the layout keeps it (`center` for anything else).
pub fn align_static(align: &str) -> &'static str {
    pick(Some(align), &ALIGNS).unwrap_or(DEFAULT.align)
}

/// `wrap` as the layout keeps it (`block` for anything else).
pub fn wrap_static(wrap: &str) -> &'static str {
    pick(Some(wrap), &WRAPS).unwrap_or(DEFAULT.wrap)
}

/// `size` as the layout keeps it (`large` for anything else).
pub fn size_static(size: &str) -> &'static str {
    pick(Some(size), &SIZES).unwrap_or(DEFAULT.size)
}

/// `clampImageWidth`: a custom width rounded (halves up) and kept in 10–100.
pub fn clamp_width(width: f64) -> u32 {
    if !width.is_finite() {
        return 10;
    }
    (width + 0.5).floor().clamp(10.0, 100.0) as u32
}

/// `setAttribute` and `removeAttribute`: a changed attribute keeps its place, a new one goes
/// last.
fn set_attr(element: &mut Element, name: &str, value: Option<String>) {
    let at = element.attrs.iter().position(|(n, _)| n == name);
    match (at, value) {
        (Some(i), Some(value)) => element.attrs[i].1 = value,
        (Some(i), None) => {
            element.attrs.remove(i);
        }
        (None, Some(value)) => element.attrs.push((name.to_owned(), value)),
        (None, None) => {}
    }
}

/// `applyImageLayout` on an element: the validated attributes, the defaults left out.
fn apply(element: &mut Element, layout: &ImageLayout) {
    let size = if SIZES.contains(&layout.size) {
        layout.size
    } else {
        DEFAULT.size
    };
    let width = layout.width.map(|w| w.clamp(10, 100));
    let safe = match (size, width) {
        ("custom", None) => ImageLayout {
            size: DEFAULT.size,
            width: None,
            ..*layout
        },
        _ => ImageLayout {
            size,
            width,
            ..*layout
        },
    };
    let safe = ImageLayout {
        align: pick(Some(safe.align), &ALIGNS).unwrap_or(DEFAULT.align),
        wrap: pick(Some(safe.wrap), &WRAPS).unwrap_or(DEFAULT.wrap),
        ..safe
    };
    let differs =
        |value: &'static str, default: &'static str| (value != default).then(|| value.to_owned());
    set_attr(element, "data-size", differs(safe.size, DEFAULT.size));
    set_attr(
        element,
        "data-width",
        (safe.size == "custom").then(|| safe.width.unwrap_or(100).to_string()),
    );
    set_attr(element, "data-align", differs(safe.align, DEFAULT.align));
    set_attr(element, "data-wrap", differs(safe.wrap, DEFAULT.wrap));
}

/// Rewrites the first `<img>` in `html` (a picture atom) with `change`.
fn edit_image(html: &str, change: impl FnOnce(&mut Element)) -> String {
    let mut nodes = super::parse_fragment(html);
    if let Some(image) = nodes.iter_mut().find_map(|n| match n {
        super::Node::Element(e) if e.name == "img" => Some(e),
        _ => None,
    }) {
        change(image);
    }
    super::serialize(&nodes)
}

/// `applySelectedImageLayout`: a picture's markup with its layout changed.
pub fn apply_to_html(html: &str, layout: &ImageLayout) -> String {
    edit_image(html, |image| apply(image, layout))
}

/// `setSelectedImageWidth`: the picture's `width` and `height` (both removed for its original
/// size), and its column width back to the default size.
pub fn set_pixel_size(html: &str, size: Option<(u32, u32)>) -> String {
    edit_image(html, |image| {
        set_attr(image, "width", size.map(|(w, _)| w.to_string()));
        set_attr(image, "height", size.map(|(_, h)| h.to_string()));
        let layout = ImageLayout {
            size: DEFAULT.size,
            width: None,
            ..read(image)
        };
        apply(image, &layout);
    })
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    const IMG: &str = r#"<img src="data:image/png;base64,AA==" alt="Map">"#;

    fn layout(html: &str) -> ImageLayout {
        match super::super::parse_fragment(html).into_iter().next() {
            Some(super::super::Node::Element(e)) => read(&e),
            _ => panic!("no image"),
        }
    }

    #[test]
    fn layouts_are_written_as_applyimagelayout_writes_them() {
        let medium = apply_to_html(
            IMG,
            &ImageLayout {
                size: "medium",
                align: "left",
                ..DEFAULT
            },
        );
        assert_eq!(
            medium,
            r#"<img src="data:image/png;base64,AA==" alt="Map" data-size="medium" data-align="left">"#
        );
        let custom = apply_to_html(
            IMG,
            &ImageLayout {
                size: "custom",
                width: Some(62),
                ..DEFAULT
            },
        );
        assert_eq!(
            custom,
            r#"<img src="data:image/png;base64,AA==" alt="Map" data-size="custom" data-width="62">"#
        );
        // Back to the defaults: the stale attributes go.
        assert_eq!(apply_to_html(&custom, &DEFAULT), IMG);
        // Hostile values are validated first.
        let hostile = apply_to_html(
            IMG,
            &ImageLayout {
                size: "custom",
                width: Some(900),
                align: "left; position: fixed",
                wrap: "inline",
            },
        );
        assert_eq!(
            hostile,
            r#"<img src="data:image/png;base64,AA==" alt="Map" data-size="custom" data-width="100">"#
        );
        let round = ImageLayout {
            size: "small",
            align: "right",
            wrap: "wrap-right",
            width: None,
        };
        assert_eq!(layout(&apply_to_html(IMG, &round)), round);
    }

    #[test]
    fn pixel_sizes_reset_the_column_width() {
        let small = apply_to_html(
            IMG,
            &ImageLayout {
                size: "small",
                align: "right",
                ..DEFAULT
            },
        );
        let sized = set_pixel_size(&small, Some((340, 227)));
        assert_eq!(
            sized,
            r#"<img src="data:image/png;base64,AA==" alt="Map" data-align="right" width="340" height="227">"#
        );
        assert_eq!(
            set_pixel_size(&sized, None),
            r#"<img src="data:image/png;base64,AA==" alt="Map" data-align="right">"#
        );
        for (input, want) in [
            (5.0, 10),
            (62.4, 62),
            (62.5, 63),
            (140.0, 100),
            (f64::NAN, 10),
        ] {
            assert_eq!(clamp_width(input), want);
        }
    }
}
