//! `PublishOptions` with ProseCraft's defaults and limits, read from a project's
//! `publishOptions` (`normalizePublishOptions`, `normalizeMatterPages`).

use unicode_normalization::UnicodeNormalization;

use super::compile::MatterPage;
use crate::html::is_js_whitespace;
use crate::js::{JsString, JsValue};

/// What separates scenes in a chapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneSeparator {
    Blank,
    Asterisks,
    #[default]
    Hash,
    Line,
    None,
}

impl SceneSeparator {
    fn read(value: &str) -> Option<Self> {
        Some(match value {
            "blank" => Self::Blank,
            "asterisks" => Self::Asterisks,
            "hash" => Self::Hash,
            "line" => Self::Line,
            "none" => Self::None,
            _ => return None,
        })
    }
}

/// The publishing settings the exporters use.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishOptions {
    /// A built-in style id or a custom style's id.
    pub style: String,
    /// Whether the style sets chapter headings in capitals.
    pub chapter_heading_uppercase: bool,
    /// The style's words per page for the page estimate (0: reflowable). `None` for a custom
    /// style, whose estimate from its page size isn't ported yet.
    pub words_per_page: Option<u32>,
    /// The chosen format's id (`pdf`, `epub`, `text`…).
    pub format: String,
    pub include_title_page: bool,
    pub include_copyright_page: bool,
    pub include_table_of_contents: bool,
    pub include_chapter_titles: bool,
    pub include_scene_titles: bool,
    pub page_break_before_chapters: bool,
    pub scene_separator: SceneSeparator,
    pub drop_caps: bool,
    pub footnotes_restart_per_chapter: bool,
    pub subtitle: String,
    pub copyright: String,
    pub dedication: String,
    pub front_matter: Vec<MatterPage>,
    pub back_matter: Vec<MatterPage>,
}

impl Default for PublishOptions {
    /// `defaultPublishOptions`.
    fn default() -> Self {
        Self {
            style: "manuscript".into(),
            chapter_heading_uppercase: true,
            words_per_page: Some(250),
            format: "pdf".into(),
            include_title_page: true,
            include_copyright_page: false,
            include_table_of_contents: false,
            include_chapter_titles: true,
            include_scene_titles: false,
            page_break_before_chapters: true,
            scene_separator: SceneSeparator::Hash,
            drop_caps: true,
            footnotes_restart_per_chapter: false,
            subtitle: String::new(),
            copyright: String::new(),
            dedication: String::new(),
            front_matter: Vec::new(),
            back_matter: Vec::new(),
        }
    }
}

/// The built-in styles and whether each sets chapter headings in capitals (`PRESETS`).
/// The built-in styles: id, capital chapter headings, words per page (`PRESETS`).
const BUILTIN_STYLES: [(&str, bool, u32); 4] = [
    ("manuscript", true, 250),
    ("classic", true, 330),
    ("modern", false, 400),
    ("ebook", false, 0),
];

/// Every format ProseCraft offers (`FORMAT_ORDER` and `SCREENPLAY_FORMAT_ORDER`).
pub const FORMATS: [&str; 8] = [
    "pdf",
    "epub",
    "docx",
    "markdown",
    "html",
    "text",
    "fountain",
    "screenplay-pdf",
];

/// `value.slice(0, n)` on a JavaScript string: the first `n` UTF-16 code units.
fn js_slice(text: &JsString, n: usize) -> String {
    JsString::from_units(text.units().iter().take(n).copied().collect()).to_string_lossy()
}

fn text_field(value: Option<&JsValue>, limit: usize) -> Option<String> {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| js_slice(s, limit))
}

fn flag(value: Option<&JsValue>, default: bool) -> bool {
    match value {
        Some(JsValue::Bool(b)) => *b,
        _ => default,
    }
}

/// `normalizeMatterPages`: at most 12 pages, titles 120 and bodies 20,000 characters.
fn matter_pages(value: Option<&JsValue>) -> Vec<MatterPage> {
    let mut pages: Vec<MatterPage> = Vec::new();
    for (index, entry) in value
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        if entry.as_object().is_none() {
            continue;
        }
        let id = text_field(entry.get("id"), usize::MAX)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let id = match id {
            Some(id) if !pages.iter().any(|p| p.id == id) => id,
            // ProseCraft makes a random id here; exports only need it to be unique.
            _ => format!("matter-{index}"),
        };
        pages.push(MatterPage {
            id,
            title: text_field(entry.get("title"), 120).unwrap_or_default(),
            body: text_field(entry.get("body"), 20_000).unwrap_or_default(),
            blank: matches!(entry.get("blank"), Some(JsValue::Bool(true))),
        });
        if pages.len() >= 12 {
            break;
        }
    }
    pages
}

impl PublishOptions {
    /// `normalizePublishOptions`: a project's `publishOptions`, with defaults for whatever is
    /// missing or invalid.
    pub fn read(value: Option<&JsValue>) -> Self {
        let defaults = Self::default();
        let Some(value) = value.filter(|v| v.as_object().is_some()) else {
            return defaults;
        };
        let custom: Vec<(String, bool)> = value
            .get("customStyles")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|s| {
                let id = s
                    .get("id")
                    .and_then(JsValue::as_js_string)?
                    .to_string_lossy();
                Some((id, flag(s.get("chapterHeadingUppercase"), false)))
            })
            .collect();
        let requested = value
            .get("style")
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy);
        let builtin = requested
            .as_deref()
            .and_then(|s| BUILTIN_STYLES.iter().find(|(id, ..)| *id == s));
        let custom_style = requested
            .as_deref()
            .and_then(|s| custom.iter().find(|(id, _)| id == s));
        let (style, uppercase, words_per_page) = match (builtin, custom_style) {
            (Some((id, upper, words)), _) => ((*id).to_owned(), *upper, Some(*words)),
            (None, Some((id, upper))) => (id.clone(), *upper, None),
            _ => (
                defaults.style.clone(),
                defaults.chapter_heading_uppercase,
                defaults.words_per_page,
            ),
        };
        let format = value
            .get("format")
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
            .filter(|f| FORMATS.contains(&f.as_str()));
        let separator = value
            .get("sceneSeparator")
            .and_then(JsValue::as_js_string)
            .and_then(|s| SceneSeparator::read(&s.to_string_lossy()));
        Self {
            style,
            chapter_heading_uppercase: uppercase,
            words_per_page,
            format: format.unwrap_or(defaults.format),
            include_title_page: flag(value.get("includeTitlePage"), defaults.include_title_page),
            include_copyright_page: flag(
                value.get("includeCopyrightPage"),
                defaults.include_copyright_page,
            ),
            include_table_of_contents: flag(
                value.get("includeTableOfContents"),
                defaults.include_table_of_contents,
            ),
            include_chapter_titles: flag(
                value.get("includeChapterTitles"),
                defaults.include_chapter_titles,
            ),
            include_scene_titles: flag(
                value.get("includeSceneTitles"),
                defaults.include_scene_titles,
            ),
            page_break_before_chapters: flag(
                value.get("pageBreakBeforeChapters"),
                defaults.page_break_before_chapters,
            ),
            scene_separator: separator.unwrap_or(defaults.scene_separator),
            drop_caps: flag(value.get("dropCaps"), defaults.drop_caps),
            footnotes_restart_per_chapter: flag(
                value.get("footnotesRestartPerChapter"),
                defaults.footnotes_restart_per_chapter,
            ),
            subtitle: text_field(value.get("subtitle"), 200).unwrap_or_default(),
            copyright: text_field(value.get("copyright"), 500).unwrap_or_default(),
            dedication: text_field(value.get("dedication"), 1000).unwrap_or_default(),
            front_matter: matter_pages(value.get("frontMatter")),
            back_matter: matter_pages(value.get("backMatter")),
        }
    }
}

/// `slugifyFilename`: lower-case letters, digits and hyphens, at most 60 characters, or
/// "untitled". (`\p{L}` is approximated by Unicode's Alphabetic property.)
pub fn slugify_filename(value: &str) -> String {
    let kept: String = value
        .nfc()
        .filter(|c| c.is_alphabetic() || c.is_numeric() || is_js_whitespace(*c) || *c == '-')
        .collect();
    let mut slug = String::new();
    let mut in_run = false;
    for c in kept.trim_matches(is_js_whitespace).chars() {
        if is_js_whitespace(c) || c == '_' || c == '-' {
            if !in_run {
                slug.push('-');
            }
            in_run = true;
        } else {
            slug.push(c);
            in_run = false;
        }
    }
    let slug = slug.trim_matches('-').to_lowercase();
    let clipped: String = slug.chars().take(60).collect();
    let clipped = clipped.trim_end_matches('-');
    if clipped.is_empty() {
        "untitled".to_owned()
    } else {
        clipped.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{PublishOptions, SceneSeparator, slugify_filename};
    use crate::js::parse;

    #[test]
    fn reads_options_with_prosecraft_defaults() {
        let value = parse(r#"{"style":"modern","sceneSeparator":"stars","includeSceneTitles":true,"subtitle":"x"}"#).ok();
        let options = PublishOptions::read(value.as_ref());
        assert_eq!(options.style, "modern");
        assert!(!options.chapter_heading_uppercase);
        assert_eq!(options.scene_separator, SceneSeparator::Hash);
        assert!(options.include_scene_titles && options.include_title_page);
        let unknown = PublishOptions::read(parse(r#"{"style":"nope"}"#).ok().as_ref());
        assert_eq!(
            (unknown.style.as_str(), unknown.chapter_heading_uppercase),
            ("manuscript", true)
        );
    }

    #[test]
    fn slugs_like_prosecraft() {
        assert_eq!(
            slugify_filename("The Lighthouse 🌊 Keeper"),
            "the-lighthouse-keeper"
        );
        assert_eq!(slugify_filename("  --Ça va? _x_ "), "ça-va-x");
        assert_eq!(slugify_filename("🌊"), "untitled");
    }
}
