//! Custom export styles (ProseCraft `src/lib/publish/presets.ts`, upstream `2d1cf18`): a
//! project's own styles, kept as semantic choices (a font, sizes, a trim size, margins) and
//! turned into the same tokens the built-in styles have.

use crate::js::{JsObject, JsValue, number_to_string};

use super::html::StyleTokens;

/// `STYLE_FONTS`: id, label, CSS font family, DOCX font, PDF font.
pub const FONTS: [(&str, &str, &str, &str, &str); 8] = [
    (
        "georgia",
        "Georgia",
        "Georgia, 'Times New Roman', serif",
        "Georgia",
        "times",
    ),
    (
        "iowan",
        "Iowan Old Style",
        "Georgia, 'Iowan Old Style', 'Times New Roman', serif",
        "Georgia",
        "times",
    ),
    (
        "times",
        "Times New Roman",
        "'Times New Roman', Times, serif",
        "Times New Roman",
        "times",
    ),
    (
        "literata",
        "Literata",
        "'Literata', Georgia, serif",
        "Georgia",
        "times",
    ),
    (
        "ibm-plex-serif",
        "IBM Plex Serif",
        "'IBM Plex Serif', 'Literata', Georgia, serif",
        "Georgia",
        "times",
    ),
    (
        "ibm-plex-sans",
        "IBM Plex Sans",
        "'IBM Plex Sans', Helvetica, Arial, sans-serif",
        "Helvetica",
        "helvetica",
    ),
    (
        "helvetica",
        "Helvetica / Arial",
        "Helvetica, Arial, sans-serif",
        "Helvetica",
        "helvetica",
    ),
    (
        "courier",
        "Courier",
        "'Courier New', Courier, monospace",
        "Courier New",
        "courier",
    ),
];

/// `STYLE_TRIMS`: id, label, short label, width and height in points.
pub const TRIMS: [(&str, &str, &str, f64, f64); 7] = [
    (
        "letter",
        "US Letter (8.5 × 11 in)",
        "US Letter",
        612.0,
        792.0,
    ),
    ("a4", "A4 (210 × 297 mm)", "A4", 595.28, 841.89),
    ("a5", "A5 (148 × 210 mm)", "A5", 419.53, 595.28),
    ("trade-6x9", "6 × 9 in trade", "6 × 9 in", 432.0, 648.0),
    (
        "trade-5.5x8.5",
        "5.5 × 8.5 in digest",
        "5.5 × 8.5 in",
        396.0,
        612.0,
    ),
    ("trade-5.25x8", "5.25 × 8 in", "5.25 × 8 in", 378.0, 576.0),
    ("trade-5x8", "5 × 8 in", "5 × 8 in", 360.0, 576.0),
];

/// The built-in styles (`PRESET_ORDER`): id and label.
pub const BUILTIN: [(&str, &str); 4] = [
    ("manuscript", "Manuscript"),
    ("classic", "Classic novel"),
    ("modern", "Modern trade"),
    ("ebook", "Ebook"),
];

/// `CUSTOM_STYLE_ID_PREFIX`, `CUSTOM_STYLE_LABEL_LIMIT`, `MAX_CUSTOM_STYLES`.
pub const ID_PREFIX: &str = "custom-";
pub const LABEL_LIMIT: usize = 60;
pub const MAX_STYLES: usize = 24;

/// `CustomExportStyle`.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomStyle {
    pub id: String,
    pub label: String,
    pub font_id: &'static str,
    pub body_size: f64,
    pub line_height: f64,
    pub indent: f64,
    pub spacing: f64,
    pub justify: bool,
    pub chapter_size: f64,
    pub chapter_uppercase: bool,
    pub chapter_align: &'static str,
    pub chapter_before: f64,
    pub chapter_after: f64,
    pub scene_break: f64,
    pub drop_cap: bool,
    pub trim_id: &'static str,
    /// Margins top, right, bottom, left, in inches.
    pub margins: [f64; 4],
    pub running_header: bool,
    pub page_numbers: bool,
}

/// `clampNumber`: a number, or a numeric string, within `min..=max`; anything else `fallback`.
fn clamp_number(value: Option<&JsValue>, min: f64, max: f64, fallback: f64) -> f64 {
    let number = match value {
        Some(JsValue::Number(n)) => *n,
        Some(JsValue::String(s)) => {
            let text = s.to_string_lossy();
            if text.trim_matches(crate::html::is_js_whitespace).is_empty() {
                f64::NAN
            } else {
                crate::js::string_to_number(&text)
            }
        }
        _ => f64::NAN,
    };
    if number.is_finite() {
        number.max(min).min(max)
    } else {
        fallback
    }
}

/// `choice`.
fn choice(
    value: Option<&JsValue>,
    allowed: &[&'static str],
    fallback: &'static str,
) -> &'static str {
    let text = value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy());
    text.and_then(|t| allowed.iter().find(|a| **a == t).copied())
        .unwrap_or(fallback)
}

/// `booleanField`.
fn boolean(value: Option<&JsValue>, fallback: bool) -> bool {
    match value {
        Some(JsValue::Bool(b)) => *b,
        _ => fallback,
    }
}

fn font_ids() -> Vec<&'static str> {
    FONTS.iter().map(|f| f.0).collect()
}

fn trim_ids() -> Vec<&'static str> {
    TRIMS.iter().map(|t| t.0).collect()
}

/// `coerceCustomStyleFields`, with the id and label given.
fn coerce(entry: &JsValue, id: String, label: String) -> CustomStyle {
    let n = |key: &str, min: f64, max: f64, fallback: f64| {
        clamp_number(entry.get(key), min, max, fallback)
    };
    CustomStyle {
        id,
        label,
        font_id: choice(entry.get("fontId"), &font_ids(), "georgia"),
        body_size: n("bodySizePt", 6.0, 36.0, 11.5),
        line_height: n("lineHeight", 1.0, 3.0, 1.5),
        indent: n("paragraphIndentEm", 0.0, 4.0, 1.5),
        spacing: n("paragraphSpacingEm", 0.0, 3.0, 0.0),
        justify: boolean(entry.get("justify"), false),
        chapter_size: n("chapterHeadingSizePt", 8.0, 48.0, 18.0),
        chapter_uppercase: boolean(entry.get("chapterHeadingUppercase"), false),
        chapter_align: choice(
            entry.get("chapterHeadingAlign"),
            &["left", "center"],
            "left",
        ),
        chapter_before: n("chapterHeadingSpaceBefore", 0.0, 6.0, 1.0),
        chapter_after: n("chapterHeadingSpaceAfter", 0.0, 6.0, 1.5),
        scene_break: n("sceneBreakSpacing", 0.0, 3.0, 1.1),
        drop_cap: boolean(entry.get("dropCap"), false),
        trim_id: choice(entry.get("trimId"), &trim_ids(), "trade-6x9"),
        margins: [
            n("marginTopIn", 0.25, 1.5, 0.75),
            n("marginRightIn", 0.25, 1.5, 0.75),
            n("marginBottomIn", 0.25, 1.5, 0.75),
            n("marginLeftIn", 0.25, 1.5, 0.75),
        ],
        running_header: boolean(entry.get("runningHeader"), false),
        page_numbers: boolean(entry.get("pageNumbers"), true),
    }
}

fn utf16_prefix(text: &str, limit: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= limit
        })
        .collect()
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(crate::html::is_js_whitespace)
}

/// `normalizeCustomStyles`: the project's styles, invalid ones left out, every field clamped,
/// at most 24.
pub fn normalize_custom_styles(value: Option<&JsValue>) -> Vec<CustomStyle> {
    let mut result: Vec<CustomStyle> = Vec::new();
    for entry in value.and_then(JsValue::as_array).unwrap_or_default() {
        if entry.as_object().is_none() {
            continue;
        }
        let id = entry
            .get("id")
            .and_then(JsValue::as_js_string)
            .map(|s| js_trim(&s.to_string_lossy()).to_owned())
            .unwrap_or_default();
        let builtin = BUILTIN.iter().any(|(b, _)| *b == id);
        if id.is_empty()
            || result.iter().any(|s| s.id == id)
            || id.encode_utf16().count() > 100
            || builtin
        {
            continue;
        }
        let label = entry
            .get("label")
            .and_then(JsValue::as_js_string)
            .map(|s| utf16_prefix(js_trim(&s.to_string_lossy()), LABEL_LIMIT))
            .unwrap_or_default();
        if label.is_empty() {
            continue;
        }
        result.push(coerce(entry, id, label));
        if result.len() >= MAX_STYLES {
            break;
        }
    }
    result
}

/// `ptToInches`, `inchesToPt`.
fn pt_to_inches(points: f64) -> f64 {
    js_round(points / 72.0 * 100.0) / 100.0
}

fn inches_to_pt(inches: f64) -> f64 {
    js_round(inches * 72.0 * 10.0) / 10.0
}

/// `Math.round`: halves up.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `trimNumber`.
fn trim_number(value: f64) -> String {
    if value.fract() == 0.0 {
        return number_to_string(value);
    }
    let fixed = format!("{value:.2}");
    fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn font(
    id: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    FONTS
        .iter()
        .find(|f| f.0 == id)
        .copied()
        .unwrap_or(FONTS[0])
}

fn trim(id: &str) -> (&'static str, &'static str, &'static str, f64, f64) {
    TRIMS
        .iter()
        .find(|t| t.0 == id)
        .copied()
        .unwrap_or(TRIMS[0])
}

/// `estimateWordsPerPage`.
pub fn words_per_page(style: &CustomStyle) -> u32 {
    let (.., width, height) = trim(style.trim_id);
    let [top, right, bottom, left] = style.margins;
    let content_width = width - inches_to_pt(left) - inches_to_pt(right);
    let content_height = height - inches_to_pt(top) - inches_to_pt(bottom);
    if content_width <= 0.0 || content_height <= 0.0 {
        return 0;
    }
    let per_line = content_width / (style.body_size * 0.55);
    let lines = content_height / (style.body_size * style.line_height);
    js_round(per_line * lines / 6.2).max(1.0) as u32
}

/// The style's "detail" line (`customStyleTokens(style).detail`).
pub fn detail(style: &CustomStyle) -> String {
    format!(
        "{} pt · {} · {}",
        trim_number(style.body_size),
        font(style.font_id).1,
        trim(style.trim_id).2
    )
}

/// `customStyleTokens`.
pub fn custom_style_tokens(style: &CustomStyle) -> StyleTokens {
    let (_, _, css, docx, pdf) = font(style.font_id);
    let (.., width, height) = trim(style.trim_id);
    let [top, right, bottom, left] = style.margins;
    StyleTokens {
        font: css,
        body_size: style.body_size,
        line_height: style.line_height,
        indent: style.indent,
        spacing: style.spacing,
        justify: style.justify,
        chapter_size: style.chapter_size,
        chapter_uppercase: style.chapter_uppercase,
        chapter_align: style.chapter_align,
        chapter_before: style.chapter_before,
        chapter_after: style.chapter_after,
        scene_break: style.scene_break,
        drop_cap: style.drop_cap,
        page: [
            width,
            height,
            inches_to_pt(top),
            inches_to_pt(right),
            inches_to_pt(bottom),
            inches_to_pt(left),
        ],
        docx_font: docx,
        pdf_font: pdf,
        running_header: style.running_header,
        page_numbers: style.page_numbers,
    }
}

/// `resolveStyleTokens`: a built-in style's tokens, a custom style's, or Manuscript's.
pub fn resolve_style_tokens(style: &str, customs: &[CustomStyle]) -> StyleTokens {
    match customs.iter().find(|c| c.id == style) {
        Some(custom) if !BUILTIN.iter().any(|(b, _)| *b == style) => custom_style_tokens(custom),
        _ => super::html::style_tokens(style).clone(),
    }
}

/// `uniqueStyleLabel`.
fn unique_label(base: &str, customs: &[CustomStyle]) -> String {
    let used: Vec<String> = BUILTIN
        .iter()
        .map(|(_, l)| l.to_lowercase())
        .chain(customs.iter().map(|s| js_trim(&s.label).to_lowercase()))
        .collect();
    let clean = utf16_prefix(js_trim(base), LABEL_LIMIT);
    let clean = if clean.is_empty() {
        "Custom style".to_owned()
    } else {
        clean
    };
    if !used.contains(&clean.to_lowercase()) {
        return clean;
    }
    let mut index = 2;
    loop {
        let suffix = format!(" {index}");
        let candidate = format!(
            "{}{suffix}",
            utf16_prefix(&clean, LABEL_LIMIT - suffix.len())
        );
        if !used.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        index += 1;
    }
}

/// `customFieldsFromTokens`: a built-in style's choices.
fn from_tokens(tokens: &StyleTokens, id: String, label: String) -> CustomStyle {
    let font_id = FONTS
        .iter()
        .find(|f| f.2 == tokens.font)
        .map(|f| f.0)
        .unwrap_or(match tokens.pdf_font {
            "helvetica" => "helvetica",
            "courier" => "courier",
            _ => "georgia",
        });
    let [width, height, top, right, bottom, left] = tokens.page;
    let trim_id = TRIMS
        .iter()
        .find(|t| t.3 == width && t.4 == height)
        .map_or("trade-6x9", |t| t.0);
    CustomStyle {
        id,
        label,
        font_id,
        body_size: tokens.body_size,
        line_height: tokens.line_height,
        indent: tokens.indent,
        spacing: tokens.spacing,
        justify: tokens.justify,
        chapter_size: tokens.chapter_size,
        chapter_uppercase: tokens.chapter_uppercase,
        chapter_align: if tokens.chapter_align == "center" {
            "center"
        } else {
            "left"
        },
        chapter_before: tokens.chapter_before,
        chapter_after: tokens.chapter_after,
        scene_break: tokens.scene_break,
        drop_cap: tokens.drop_cap,
        trim_id,
        margins: [
            pt_to_inches(top),
            pt_to_inches(right),
            pt_to_inches(bottom),
            pt_to_inches(left),
        ],
        running_header: tokens.running_header,
        page_numbers: tokens.page_numbers,
    }
}

/// The label a style is shown with.
pub fn label_of(style: &str, customs: &[CustomStyle]) -> String {
    BUILTIN
        .iter()
        .find(|(b, _)| *b == style)
        .map(|(_, l)| (*l).to_owned())
        .or_else(|| {
            customs
                .iter()
                .find(|c| c.id == style)
                .map(|c| c.label.clone())
        })
        .unwrap_or_else(|| "Manuscript".to_owned())
}

/// `createCustomStyle`: a copy of `source` (a built-in or custom style), named
/// "… copy", with a new id (`custom-` and `unique`).
pub fn create_custom_style(source: &str, customs: &[CustomStyle], unique: &str) -> CustomStyle {
    let id = format!("{ID_PREFIX}{unique}");
    let label = unique_label(&format!("{} copy", label_of(source, customs)), customs);
    match customs.iter().find(|c| c.id == source) {
        Some(custom) => CustomStyle {
            id,
            label,
            ..custom.clone()
        },
        None => from_tokens(&resolve_style_tokens(source, customs), id, label),
    }
}

/// `normalizeCustomDraft`: a draft from the editor, clamped, its name trimmed (or
/// "Custom style").
pub fn normalize_draft(style: &CustomStyle) -> CustomStyle {
    let label = utf16_prefix(js_trim(&style.label), LABEL_LIMIT);
    let label = if label.is_empty() {
        "Custom style".to_owned()
    } else {
        label
    };
    coerce(&to_value(style), style.id.clone(), label)
}

/// The style as `publishOptions.customStyles` keeps it.
pub fn to_value(style: &CustomStyle) -> JsValue {
    let mut o = JsObject::new();
    o.insert("id", JsValue::str(&style.id));
    o.insert("label", JsValue::str(&style.label));
    o.insert("fontId", JsValue::str(style.font_id));
    o.insert("bodySizePt", JsValue::Number(style.body_size));
    o.insert("lineHeight", JsValue::Number(style.line_height));
    o.insert("paragraphIndentEm", JsValue::Number(style.indent));
    o.insert("paragraphSpacingEm", JsValue::Number(style.spacing));
    o.insert("justify", JsValue::Bool(style.justify));
    o.insert("chapterHeadingSizePt", JsValue::Number(style.chapter_size));
    o.insert(
        "chapterHeadingUppercase",
        JsValue::Bool(style.chapter_uppercase),
    );
    o.insert("chapterHeadingAlign", JsValue::str(style.chapter_align));
    o.insert(
        "chapterHeadingSpaceBefore",
        JsValue::Number(style.chapter_before),
    );
    o.insert(
        "chapterHeadingSpaceAfter",
        JsValue::Number(style.chapter_after),
    );
    o.insert("sceneBreakSpacing", JsValue::Number(style.scene_break));
    o.insert("dropCap", JsValue::Bool(style.drop_cap));
    o.insert("trimId", JsValue::str(style.trim_id));
    o.insert("marginTopIn", JsValue::Number(style.margins[0]));
    o.insert("marginRightIn", JsValue::Number(style.margins[1]));
    o.insert("marginBottomIn", JsValue::Number(style.margins[2]));
    o.insert("marginLeftIn", JsValue::Number(style.margins[3]));
    o.insert("runningHeader", JsValue::Bool(style.running_header));
    o.insert("pageNumbers", JsValue::Bool(style.page_numbers));
    JsValue::Object(o)
}
