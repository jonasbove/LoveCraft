//! Citations and bibliographies in ProseCraft's eight styles (`src/lib/citations/format.ts`,
//! upstream `7dae544`): APA 7, Chicago author-date and notes, MLA 9, Harvard, IEEE, Vancouver
//! and DIN 1505-2, in English and German. A line-by-line port; the styles' settings and terms
//! are ProseCraft's own, exported as data (`styles.json`).
//!
//! Bibliographies sort as `localeCompare` does, through craft-i18n's ICU collation in the root
//! order. That collation also compares runs of digits by value, which `localeCompare` without
//! `numeric` doesn't, so two titles that differ only in a number ("Part 10", "Part 2") can sort
//! the other way round.

use super::styles::{self, Template};
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsValue, number_to_string};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// What a cited source is said with: a locator and its label, words before and after, and
/// whether the author is left out.
#[derive(Clone, Debug)]
pub struct CitationInput {
    pub item: JsValue,
    pub locator: Option<String>,
    /// `page`, `chapter` or `section`; a page when none is given.
    pub label: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub suppress_author: bool,
}

/// `CitationFormatContext`.
#[derive(Clone, Debug, Default)]
pub struct CitationContext {
    /// The citation's 1-based number in numeric styles.
    pub index: Option<f64>,
    /// Per-item numbers for a numeric citation of several sources.
    pub indices: Option<HashMap<String, f64>>,
    /// `en` or `de`.
    pub locale: String,
    /// Year suffixes worked out already (`{ id: "a" }`).
    pub year_suffixes: Option<HashMap<String, String>>,
    /// The library to work year suffixes out from.
    pub library: Option<Vec<JsValue>>,
}

/// A citation or a bibliography entry, as plain text and as HTML (titles in `<i>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    pub html: String,
}

/// One entry of a bibliography.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BibliographyEntry {
    pub id: String,
    pub text: String,
    pub html: String,
}

macro_rules! rule {
    ($name:ident, $source:expr, $flags:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

rule!(ends_punctuation, r#"[.!?]["']?$"#, "");
rule!(spaces, r"\s{2,}", "");
rule!(whitespace_run, r"\s+", "");
rule!(range_value, r"\d\s*[-–—,]\s*\d", "");
rule!(
    page_range_rule,
    r"(\d[A-Za-z]?)\s*-{1,2}\s*(?=[A-Za-z]?\d)",
    ""
);
rule!(attaches, r"^[.,;:!?\])]", "");
rule!(four_digits, r"\b(\d{4})\b", "");
rule!(doi_host, r"^https?:\/\/(?:dx\.)?doi\.org\/", "i");
rule!(doi_prefix, r"^doi:\s*", "i");
rule!(digits, r"^\d+$", "");
rule!(terminal, r"[.!?]$", "");

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

#[derive(Clone, Debug)]
struct Span {
    italic: bool,
    text: String,
}

/// `SpanBuilder`: text in runs, plain or italic.
#[derive(Default)]
struct Spans(Vec<Span>);

impl Spans {
    fn push(&mut self, text: &str) -> &mut Self {
        self.push_as(text, false)
    }

    fn push_italic(&mut self, text: &str) -> &mut Self {
        self.push_as(text, true)
    }

    fn push_as(&mut self, text: &str, italic: bool) -> &mut Self {
        if text.is_empty() {
            return self;
        }
        match self.0.last_mut() {
            Some(last) if last.italic == italic => last.text.push_str(text),
            _ => self.0.push(Span {
                italic,
                text: text.to_owned(),
            }),
        }
        self
    }

    fn push_spans(&mut self, spans: &[Span]) -> &mut Self {
        for span in spans {
            self.push_as(&span.text, span.italic);
        }
        self
    }

    fn is_empty(&self) -> bool {
        self.0.iter().all(|s| s.text.is_empty())
    }

    fn raw(&self) -> String {
        self.0.iter().map(|s| s.text.as_str()).collect()
    }

    fn ensure_period(&mut self) -> &mut Self {
        if self.is_empty() {
            return self;
        }
        let raw = self.raw();
        if !ends_punctuation().test(raw.trim_end_matches(is_js_whitespace)) {
            self.push(".");
        }
        self
    }

    fn build(&self) -> Formatted {
        let squeeze = |s: &str| js_trim(&spaces().replace(s, true, " ")).to_owned();
        let html: String = self
            .0
            .iter()
            .map(|span| {
                let escaped = escape_html(&span.text);
                if span.italic {
                    format!("<i>{escaped}</i>")
                } else {
                    escaped
                }
            })
            .collect();
        Formatted {
            text: squeeze(&self.raw()),
            html: squeeze(&html),
        }
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A field as JavaScript would print it when it is truthy: a non-empty string, or a number
/// other than zero.
fn truthy_text(value: Option<&JsValue>) -> Option<String> {
    match value? {
        JsValue::String(s) if !s.is_empty() => Some(s.to_string_lossy()),
        JsValue::Number(n) if *n != 0.0 && !n.is_nan() => Some(number_to_string(*n)),
        _ => None,
    }
}

/// A name of a contributor (`CslName`).
#[derive(Clone, Debug, Default)]
struct Name {
    family: Option<String>,
    given: Option<String>,
    literal: Option<String>,
    non_dropping: Option<String>,
    dropping: Option<String>,
    suffix: Option<String>,
}

/// A source (`CslItem`), read from its JSON.
struct Item<'a>(&'a JsObject);

impl Item<'_> {
    fn field(&self, key: &str) -> Option<String> {
        truthy_text(self.0.get(key))
    }

    fn string(&self, key: &str) -> Option<String> {
        self.0
            .get(key)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    }

    fn id(&self) -> String {
        self.string("id").unwrap_or_default()
    }

    fn kind(&self) -> Kind {
        normalize_item_type(&self.string("type").unwrap_or_default())
    }

    /// `item.title?.trim()`, kept only when non-empty.
    fn title(&self) -> Option<String> {
        self.string("title")
            .map(|t| js_trim(&t).to_owned())
            .filter(|t| !t.is_empty())
    }

    fn names(&self, key: &str) -> Vec<Name> {
        let text = |o: &JsObject, k: &str| {
            o.get(k)
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
        };
        self.0
            .get(key)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(JsValue::as_object)
            .map(|o| Name {
                family: text(o, "family"),
                given: text(o, "given"),
                literal: text(o, "literal"),
                non_dropping: text(o, "non-dropping-particle"),
                dropping: text(o, "dropping-particle"),
                suffix: text(o, "suffix"),
            })
            .collect()
    }
}

fn nonempty(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|s| !s.is_empty())
}

/// `[a, b, …].filter(Boolean).join(sep)`.
fn join_present(parts: &[Option<&str>], sep: &str) -> String {
    parts
        .iter()
        .filter_map(|p| p.filter(|s| !s.is_empty()))
        .collect::<Vec<_>>()
        .join(sep)
}

fn family_with_particle(name: &Name) -> String {
    join_present(&[nonempty(&name.non_dropping), nonempty(&name.family)], " ")
}

/// `getInTextName`.
fn in_text_name(name: &Name) -> String {
    let family = family_with_particle(name);
    if !family.is_empty() {
        return family;
    }
    nonempty(&name.literal)
        .or(nonempty(&name.given))
        .unwrap_or_default()
        .to_owned()
}

/// `firstCharUpper`.
fn first_char_upper(token: &str) -> String {
    js_trim(token)
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// `formatInitials`.
fn format_initials(given: Option<&str>, with_dots: bool) -> String {
    let Some(given) = given.filter(|g| !g.is_empty()) else {
        return String::new();
    };
    let trimmed = js_trim(given);
    let mut formatted = Vec::new();
    let mut tokens: Vec<&str> = Vec::new();
    let mut last = 0;
    for m in whitespace_run().all(trimmed) {
        tokens.push(&trimmed[last..m.start()]);
        last = m.end();
    }
    tokens.push(&trimmed[last..]);
    for token in tokens.into_iter().filter(|t| !t.is_empty()) {
        if token.contains('-') {
            let parts: Vec<String> = token
                .split('-')
                .map(first_char_upper)
                .filter(|p| !p.is_empty())
                .collect();
            if !parts.is_empty() {
                formatted.push(if with_dots {
                    parts
                        .iter()
                        .map(|c| format!("{c}."))
                        .collect::<Vec<_>>()
                        .join("-")
                } else {
                    parts.concat()
                });
            }
        } else {
            let initial = first_char_upper(token);
            if !initial.is_empty() {
                formatted.push(if with_dots {
                    format!("{initial}.")
                } else {
                    initial
                });
            }
        }
    }
    formatted.join(if with_dots { " " } else { "" })
}

/// `formatSingleName`.
fn format_single_name(name: &Name, mode: &str) -> String {
    let literal = nonempty(&name.literal);
    if let Some(literal) = literal
        && nonempty(&name.family).is_none()
        && nonempty(&name.given).is_none()
    {
        return literal.to_owned();
    }
    let family = family_with_particle(name);
    if family.is_empty()
        && let Some(literal) = literal
    {
        return literal.to_owned();
    }
    let given = name.given.as_deref().unwrap_or_default();
    let dropping = name.dropping.as_deref().unwrap_or_default();
    let given_with_dropping = join_present(&[Some(given), Some(dropping)], " ");
    let suffix = nonempty(&name.suffix);
    let fam = Some(family.as_str());
    match mode {
        "family-given" => join_present(&[fam, Some(&given_with_dropping), suffix], ", "),
        "given-family" => {
            let core = join_present(&[Some(given), Some(dropping), fam], " ");
            match suffix {
                Some(s) => format!("{core}, {s}"),
                None => core,
            }
        }
        "family-initials-dots" => {
            let initials = format_initials(Some(given), true);
            join_present(&[fam, Some(&initials), suffix], ", ")
        }
        "initials-family" => {
            let initials = format_initials(Some(given), true);
            let core = join_present(&[Some(&initials), Some(dropping), fam], " ");
            match suffix {
                Some(s) => format!("{core}, {s}"),
                None => core,
            }
        }
        _ => {
            let initials = format_initials(Some(given), false);
            let core = join_present(&[fam, Some(&initials)], " ");
            match suffix {
                Some(s) => format!("{core} {s}"),
                None => core,
            }
        }
    }
}

/// `formatInTextAuthors`.
fn in_text_authors(
    names: &[Name],
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    if names.len() as f64 >= template.in_text_n("etAlThreshold") {
        return format!("{} {}", in_text_name(first), terms.get("etAl"));
    }
    if names.len() == 1 {
        return in_text_name(first);
    }
    let conj = if template.in_text_b("useAmpersand") {
        "&".to_owned()
    } else {
        terms.get("and")
    };
    if names.len() == 2 {
        return format!("{} {conj} {}", in_text_name(first), in_text_name(&names[1]));
    }
    let oxford = template.names_b("oxfordComma") && locale == "en";
    let head: Vec<String> = names[..names.len() - 1].iter().map(in_text_name).collect();
    let last = in_text_name(&names[names.len() - 1]);
    format!(
        "{}{} {conj} {last}",
        head.join(", "),
        if oxford { "," } else { "" }
    )
}

/// `formatNoteAuthors`.
fn note_authors(
    names: &[Name],
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    if names.len() as f64 >= template.in_text_n("etAlThreshold") {
        return format!(
            "{} {}",
            format_single_name(first, "given-family"),
            terms.get("etAl")
        );
    }
    let formatted: Vec<String> = names
        .iter()
        .map(|n| format_single_name(n, "given-family"))
        .collect();
    let and = terms.get("and");
    match formatted.as_slice() {
        [one] => one.clone(),
        [a, b] => format!("{a} {and} {b}"),
        [head @ .., last] => format!(
            "{}{} {and} {last}",
            head.join(", "),
            if locale == "en" { "," } else { "" }
        ),
        [] => String::new(),
    }
}

/// `formatEditorSuffix`.
fn editor_suffix(count: usize, kind: &str, terms: &styles::Terms) -> String {
    let plural = count > 1;
    let pick = |p: &str, s: &str| terms.get(if plural { p } else { s });
    match kind {
        "paren-cap" => format!(" {}", pick("editorParenPlural", "editorParenSingular")),
        "paren-lower" => format!(
            " {}",
            pick("editorParenLowerPlural", "editorParenLowerSingular")
        ),
        "comma-ed" => format!(", {}", pick("editorAbbrevPlural", "editorAbbrevSingular")),
        _ => format!(", {}", pick("editorFullPlural", "editorFullSingular")),
    }
}

/// `formatBibNameList`.
fn bib_name_list(
    names: &[Name],
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    editor_role: bool,
) -> String {
    if names.is_empty() {
        return String::new();
    }
    let first_mode = template.names_s("firstAuthor");
    let other_mode = template.names_s("otherAuthors");
    let separator = template.names_s("separator");
    let mode = |i: usize| if i == 0 { &first_mode } else { &other_mode };
    let threshold = template.names_n("etAlThreshold");
    let keep = template.names_n("etAlKeep").max(0.0) as usize;
    let mut rendered = if names.len() as f64 >= threshold {
        let head = names
            .iter()
            .take(keep)
            .enumerate()
            .map(|(i, n)| format_single_name(n, mode(i)))
            .collect::<Vec<_>>()
            .join(&separator);
        if template.names_b("etAlEllipsisBeforeLast") {
            let last = format_single_name(&names[names.len() - 1], &other_mode);
            format!("{head}, ... {last}")
        } else {
            let comma = template.names_b("commaBeforeEtAl");
            format!(
                "{head}{}{}",
                if comma { ", " } else { " " },
                terms.get("etAl")
            )
        }
    } else if names.len() == 1 {
        format_single_name(&names[0], &first_mode)
    } else {
        let formatted: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, n)| format_single_name(n, mode(i)))
            .collect();
        let head = formatted[..formatted.len() - 1].join(&separator);
        let last = &formatted[formatted.len() - 1];
        match template.names_s("lastSeparator").as_str() {
            "comma" => formatted.join(", "),
            "semicolon" => formatted.join(&separator),
            "ampersand" => format!("{head}, & {last}"),
            _ if formatted.len() == 2 => {
                let comma = first_mode == "family-given";
                format!(
                    "{}{} {} {last}",
                    formatted[0],
                    if comma { "," } else { "" },
                    terms.get("and")
                )
            }
            _ => {
                let oxford = template.names_b("oxfordComma") && locale == "en";
                format!(
                    "{head}{} {} {last}",
                    if oxford { "," } else { "" },
                    terms.get("and")
                )
            }
        }
    };
    if editor_role {
        rendered.push_str(&editor_suffix(
            names.len(),
            &template.names_s("primaryEditorSuffix"),
            terms,
        ));
    }
    rendered
}

/// `formatContainerEditors`.
fn container_editors(editors: &[Name], mode: &str, terms: &styles::Terms, locale: &str) -> String {
    if editors.is_empty() {
        return String::new();
    }
    let plural = editors.len() > 1;
    let and = terms.get("and");
    let join_and = |list: &[String], oxford: bool| match list {
        [one] => one.clone(),
        [a, b] => format!("{a} {and} {b}"),
        [head @ .., last] => format!(
            "{}{} {and} {last}",
            head.join(", "),
            if oxford { "," } else { "" }
        ),
        [] => String::new(),
    };
    let names =
        |m: &str| -> Vec<String> { editors.iter().map(|e| format_single_name(e, m)).collect() };
    let pick = |p: &str, s: &str| terms.get(if plural { p } else { s });
    match mode {
        "initials-ed-paren" => {
            let list = names("initials-family");
            let joined = match list.as_slice() {
                [a, b] => format!("{a} & {b}"),
                [head @ .., last] if list.len() > 2 => {
                    format!("{}, & {last}", head.join(", "))
                }
                _ => list[0].clone(),
            };
            format!(
                "{joined} {}",
                pick("editorParenPlural", "editorParenSingular")
            )
        }
        "edited-by" => format!(
            "{} {}",
            terms.get("editedBy"),
            join_and(&names("given-family"), locale == "en")
        ),
        "ed-abbrev" => format!(
            "{} {}",
            terms.get("noteEdAbbrev"),
            join_and(&names("given-family"), locale == "en")
        ),
        "family-initials-ed-paren" => format!(
            "{} {}",
            join_and(&names("family-initials-dots"), false),
            pick("editorParenLowerPlural", "editorParenLowerSingular")
        ),
        "initials-ed-cap" => {
            let paren = pick("editorParenPlural", "editorParenSingular");
            let inner: String = {
                let chars: Vec<char> = paren.chars().collect();
                if chars.len() >= 2 {
                    chars[1..chars.len() - 1].iter().collect()
                } else {
                    String::new()
                }
            };
            let label = if locale == "en" {
                if plural { "Eds." } else { "Ed." }.to_owned()
            } else {
                inner
            };
            format!(
                "{}, {label}",
                join_and(&names("initials-family"), locale == "en")
            )
        }
        "compact-editor" => format!(
            "{}, {}",
            names("family-initials-compact").join(", "),
            pick("editorFullPlural", "editorFullSingular")
        ),
        _ => format!(
            "{} {}",
            names("family-given").join(" ; "),
            pick("editorParenPlural", "editorParenSingular")
        ),
    }
}

/// `extractYear`.
fn extract_year(item: &Item) -> Option<String> {
    let issued = item.0.get("issued");
    let first = issued
        .and_then(|i| i.get("date-parts"))
        .and_then(JsValue::as_array)
        .and_then(|p| p.first())
        .and_then(JsValue::as_array)
        .and_then(|g| g.first());
    let text = match first {
        Some(JsValue::Number(n)) => Some(number_to_string(*n)),
        Some(JsValue::String(s)) => Some(s.to_string_lossy()),
        Some(JsValue::Bool(b)) => Some(b.to_string()),
        Some(JsValue::Null) => Some("null".into()),
        _ => None,
    };
    if let Some(text) = text
        && !js_trim(&text).is_empty()
    {
        return Some(js_trim(&text).to_owned());
    }
    let raw = issued.and_then(|i| {
        let get = |k: &str| {
            i.get(k)
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
        };
        get("literal").or_else(|| get("raw"))
    })?;
    if raw.is_empty() {
        return None;
    }
    Some(match four_digits().exec(&raw) {
        Some(m) => m.at(1).to_owned(),
        None => js_trim(&raw).to_owned(),
    })
}

/// `formatYear`.
fn format_year(item: &Item, terms: &styles::Terms, suffix: Option<&str>) -> String {
    let suffix = suffix.filter(|s| !s.is_empty());
    match (extract_year(item), suffix) {
        (Some(year), Some(s)) => format!("{year}{s}"),
        (Some(year), None) => year,
        (None, Some(s)) => format!("{}-{s}", terms.get("noDate")),
        (None, None) => terms.get("noDate"),
    }
}

/// `formatPageRange`: an en dash between page numbers.
fn page_range(value: &str) -> String {
    page_range_rule().replace(value, true, "$1\u{2013}")
}

/// `formatPageLabel`.
fn page_label(page: &str, terms: &styles::Terms) -> String {
    let normalized = page_range(js_trim(page));
    let label = if range_value().test(&normalized) {
        terms.get("pagePlural")
    } else {
        terms.get("pageSingle")
    };
    format!("{label} {normalized}")
}

/// `formatLocator`.
fn format_locator(
    locator: &str,
    label: &str,
    omit_page_label: bool,
    terms: &styles::Terms,
) -> String {
    let trimmed = page_range(js_trim(locator));
    if trimmed.is_empty() {
        return String::new();
    }
    match label {
        "page" if omit_page_label => trimmed,
        "page" => page_label(&trimmed, terms),
        "chapter" => format!("{} {trimmed}", terms.get("chapter")),
        _ => format!("{} {trimmed}", terms.get("section")),
    }
}

/// `ordinalEn`.
fn ordinal_en(n: f64) -> String {
    let text = number_to_string(n);
    let mod100 = n % 100.0;
    if (11.0..=13.0).contains(&mod100) {
        return format!("{text}th");
    }
    match (n % 10.0) as i64 {
        1 => format!("{text}st"),
        2 => format!("{text}nd"),
        3 => format!("{text}rd"),
        _ => format!("{text}th"),
    }
}

/// `formatEdition`.
fn format_edition(
    raw: Option<&str>,
    terms: &styles::Terms,
    locale: &str,
    harvard: bool,
) -> Option<String> {
    let trimmed = js_trim(raw.filter(|r| !r.is_empty())?);
    if trimmed.is_empty() {
        return None;
    }
    if digits().test(trimmed) {
        let n = crate::js::string_to_number(trimmed);
        if locale == "de" {
            return Some(format!(
                "{}. {}",
                number_to_string(n),
                terms.get("editionSuffix")
            ));
        }
        let suffix = terms.get(if harvard {
            "editionSuffixHarvard"
        } else {
            "editionSuffix"
        });
        return Some(format!("{} {suffix}", ordinal_en(n)));
    }
    Some(trimmed.to_owned())
}

/// `normalizeDoi`.
fn normalize_doi(raw: &str) -> String {
    let bare = doi_host().replace(js_trim(raw), false, "");
    let bare = doi_prefix().replace(&bare, false, "");
    js_trim(&bare).to_owned()
}

/// `formatDoiOrUrl`: the DOI as a link, `doi: …` or `DOI: …`, else the URL.
fn doi_or_url(item: &Item, format: &str) -> Option<String> {
    if let Some(doi) = item.field("DOI") {
        let bare = normalize_doi(&doi);
        if !bare.is_empty() {
            return Some(match format {
                "doi-lower" => format!("doi: {bare}"),
                "doi-upper" => format!("DOI: {bare}"),
                _ => format!("https://doi.org/{bare}"),
            });
        }
    }
    item.string("URL")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Book,
    Article,
    Chapter,
    Paper,
    Webpage,
    Report,
    Thesis,
}

/// `normalizeItemType`.
fn normalize_item_type(raw: &str) -> Kind {
    match raw.to_lowercase().as_str() {
        "article-journal" | "article-magazine" | "article-newspaper" | "article" => Kind::Article,
        "chapter" | "entry-encyclopedia" | "entry-dictionary" => Kind::Chapter,
        "paper-conference" => Kind::Paper,
        "webpage" | "post-weblog" | "post" => Kind::Webpage,
        "report" | "manuscript" => Kind::Report,
        "thesis" => Kind::Thesis,
        _ => Kind::Book,
    }
}

fn is_standalone_italic(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Book | Kind::Report | Kind::Thesis | Kind::Webpage
    )
}

/// `appendQuoted`.
fn append_quoted(sb: &mut Spans, text: &str, trailing: &str, inside: bool, quote: &str) {
    let trimmed = js_trim(text);
    let punct = if terminal().test(trimmed) && trailing == "." {
        ""
    } else {
        trailing
    };
    if inside {
        sb.push(&format!("{quote}{trimmed}{punct}{quote}"));
    } else {
        sb.push(&format!("{quote}{trimmed}{quote}{punct}"));
    }
}

/// `formatPlacePublisher`.
fn place_publisher(item: &Item, spacing: &str) -> Option<String> {
    let place = item
        .string("publisher-place")
        .map(|p| js_trim(&p).to_owned())
        .filter(|p| !p.is_empty());
    let publisher = item
        .string("publisher")
        .map(|p| js_trim(&p).to_owned())
        .filter(|p| !p.is_empty());
    if spacing == "publisher-only" {
        return publisher.or(place);
    }
    match (place, publisher) {
        (Some(place), Some(publisher)) => Some(if spacing == "spaced" {
            format!("{place} : {publisher}")
        } else {
            format!("{place}: {publisher}")
        }),
        (place, publisher) => publisher.or(place),
    }
}

/// `getPrimaryContributors`: the authors, else the editors.
fn primary_contributors(item: &Item) -> (Vec<Name>, bool) {
    let authors = item.names("author");
    if !authors.is_empty() {
        return (authors, false);
    }
    let editors = item.names("editor");
    if !editors.is_empty() {
        return (editors, true);
    }
    (Vec::new(), false)
}

/// `getSortKey`.
fn sort_key(item: &Item, template: &Template, terms: &styles::Terms, locale: &str) -> String {
    let (names, _) = primary_contributors(item);
    if names.is_empty() {
        return item.string("title").unwrap_or_default();
    }
    bib_name_list(&names, template, terms, locale, false)
}

/// `compareStrings`: `localeCompare` at base strength, then in full.
fn compare_strings(a: &str, b: &str) -> Ordering {
    crate::import::markdown_folder::natural_compare(a, b)
}

/// `sortBibliographyItems`: each source once, in citation order or by author, year and title.
fn sort_items<'a>(
    items: &'a [JsValue],
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
) -> Vec<&'a JsObject> {
    let mut seen = HashSet::new();
    let mut unique: Vec<&JsObject> = Vec::new();
    for item in items.iter().filter_map(JsValue::as_object) {
        if seen.insert(Item(item).id()) {
            unique.push(item);
        }
    }
    if template.bib_s("sort") == "citation-order" {
        return unique;
    }
    unique.sort_by(|a, b| {
        let (a, b) = (Item(a), Item(b));
        let keys = compare_strings(
            &sort_key(&a, template, terms, locale),
            &sort_key(&b, template, terms, locale),
        );
        if keys != Ordering::Equal {
            return keys;
        }
        let (ya, yb) = (
            extract_year(&a).unwrap_or_default(),
            extract_year(&b).unwrap_or_default(),
        );
        if ya != yb {
            if ya.is_empty() {
                return Ordering::Less;
            }
            if yb.is_empty() {
                return Ordering::Greater;
            }
            let years = compare_strings(&ya, &yb);
            if years != Ordering::Equal {
                return years;
            }
        }
        let titles = compare_strings(
            &a.string("title").unwrap_or_default(),
            &b.string("title").unwrap_or_default(),
        );
        if titles != Ordering::Equal {
            return titles;
        }
        compare_strings(&a.id(), &b.id())
    });
    unique
}

/// `indexToSuffix`: a, b, … z, aa, ab, …
fn index_to_suffix(index: usize) -> String {
    let mut n = index as i64;
    let mut out = Vec::new();
    loop {
        out.push(char::from(b'a' + (n % 26) as u8));
        n = n / 26 - 1;
        if n < 0 {
            break;
        }
    }
    out.iter().rev().collect()
}

/// `computeDisambiguation`: year suffixes (`a`, `b`, …) for sources sharing an in-text author
/// and a year, in author-date styles.
pub fn compute_disambiguation(
    style: &str,
    items: &[JsValue],
    locale: &str,
) -> HashMap<String, String> {
    let template = styles::template(style);
    if !template.in_text_b("disambiguateYear") {
        return HashMap::new();
    }
    let terms = styles::terms(locale);
    let sorted = sort_items(items, &template, &terms, locale);
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for item in sorted {
        let item = Item(item);
        let (names, _) = primary_contributors(&item);
        let author = if names.is_empty() {
            item.string("title").unwrap_or_else(|| item.id())
        } else {
            in_text_authors(&names, &template, &terms, locale)
        };
        let year = extract_year(&item).unwrap_or_else(|| terms.get("noDate"));
        let key = format!("{}||{}", author.to_lowercase(), year.to_lowercase());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, ids)) => ids.push(item.id()),
            None => groups.push((key, vec![item.id()])),
        }
    }
    let mut suffixes = HashMap::new();
    for (_, ids) in groups.into_iter().filter(|(_, ids)| ids.len() > 1) {
        for (i, id) in ids.into_iter().enumerate() {
            suffixes.insert(id, index_to_suffix(i));
        }
    }
    suffixes
}

/// `appendInTextTitle`.
fn in_text_title(sb: &mut Spans, item: &Item, template: &Template) {
    let title = item.title().unwrap_or_else(|| item.id());
    let kind = item.kind();
    if is_standalone_italic(kind) && template.in_text_b("italicizeStandaloneTitle") {
        sb.push_italic(&title);
    } else if matches!(kind, Kind::Article | Kind::Chapter | Kind::Paper) {
        sb.push(&format!("\"{title}\""));
    } else {
        sb.push(&title);
    }
}

fn push_prefix(sb: &mut Spans, prefix: Option<&str>) {
    if let Some(prefix) = prefix.filter(|p| !p.is_empty()) {
        if prefix.ends_with(' ') {
            sb.push(prefix);
        } else {
            sb.push(&format!("{prefix} "));
        }
    }
}

fn push_suffix(sb: &mut Spans, suffix: Option<&str>) {
    if let Some(suffix) = suffix.filter(|s| !s.is_empty()) {
        let trimmed = js_trim(suffix);
        if !trimmed.is_empty() {
            if attaches().test(trimmed) {
                sb.push(trimmed);
            } else {
                sb.push(&format!(" {trimmed}"));
            }
        }
    }
}

/// The notes style's journal article.
fn note_article(
    sb: &mut Spans,
    item: &Item,
    title: &str,
    year: &str,
    locator: Option<&str>,
    label: &str,
    terms: &styles::Terms,
) {
    if !title.is_empty() {
        append_quoted(sb, title, ",", terms.b("quotePunctuationInside"), "\"");
        sb.push(" ");
    }
    if let Some(c) = item.field("container-title") {
        sb.push_italic(&c);
    }
    let mut vol_issue = Vec::new();
    if let Some(v) = item.field("volume") {
        vol_issue.push(v);
    }
    if let Some(i) = item.field("issue") {
        vol_issue.push(format!("{} {i}", terms.get("issueWord")));
    }
    if !vol_issue.is_empty() {
        sb.push(&format!(" {}", vol_issue.join(", ")));
    }
    sb.push(&format!(" ({year})"));
    let page = item.field("page");
    match locator {
        Some(locator) if label == "page" => {
            sb.push(&format!(": {}", page_range(js_trim(locator))));
        }
        Some(locator) => {
            if let Some(p) = &page {
                sb.push(&format!(": {}", page_range(js_trim(p))));
            }
            sb.push(&format!(
                ", {}",
                format_locator(locator, label, true, terms)
            ));
        }
        None => {
            if let Some(p) = &page {
                sb.push(&format!(": {}", page_range(js_trim(p))));
            }
        }
    }
    if let Some(link) = doi_or_url(item, "url").filter(|l| !l.is_empty()) {
        sb.push(&format!(", {link}"));
    }
}

/// The notes style's chapter or conference paper.
#[allow(clippy::too_many_arguments)]
fn note_chapter(
    sb: &mut Spans,
    item: &Item,
    title: &str,
    year: &str,
    locator: Option<&str>,
    label: &str,
    (terms, locale): (&styles::Terms, &str),
    is_editor: bool,
) {
    if !title.is_empty() {
        append_quoted(sb, title, ",", terms.b("quotePunctuationInside"), "\"");
        sb.push(" ");
    }
    if let Some(c) = item.field("container-title") {
        sb.push(&format!("{} ", terms.get("inWord").to_lowercase()));
        sb.push_italic(&c);
    }
    let editors = item.names("editor");
    if !editors.is_empty() && !is_editor {
        sb.push(&format!(
            ", {}",
            container_editors(&editors, "ed-abbrev", terms, locale)
        ));
    }
    let publisher = place_publisher(item, "tight");
    let paren = join_present(&[publisher.as_deref(), Some(year)], ", ");
    if !paren.is_empty() {
        sb.push(&format!(" ({paren})"));
    }
    if let Some(locator) = locator {
        sb.push(&format!(
            ", {}",
            format_locator(locator, label, true, terms)
        ));
    } else if let Some(p) = item.field("page") {
        sb.push(&format!(", {}", page_range(js_trim(&p))));
    }
    if let Some(link) = doi_or_url(item, "url").filter(|l| !l.is_empty()) {
        sb.push(&format!(", {link}"));
    }
}

/// `formatChicagoNoteItem`.
fn chicago_note_item(
    entry: &CitationInput,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
) -> Vec<Span> {
    let empty = JsObject::new();
    let item = Item(entry.item.as_object().unwrap_or(&empty));
    let label = entry.label.as_deref().unwrap_or("page");
    let locator = entry.locator.as_deref().filter(|l| !l.is_empty());
    let mut sb = Spans::default();
    push_prefix(&mut sb, entry.prefix.as_deref());
    let kind = item.kind();
    let (names, is_editor) = primary_contributors(&item);
    let year = format_year(&item, terms, None);
    let title = item.title().unwrap_or_default();
    let inside = terms.b("quotePunctuationInside");
    if !entry.suppress_author && !names.is_empty() {
        let mut author = note_authors(&names, template, terms, locale);
        if is_editor {
            author.push_str(&format!(
                ", {}",
                terms.get(if names.len() > 1 {
                    "editorAbbrevPlural"
                } else {
                    "editorAbbrevSingular"
                })
            ));
        }
        sb.push(&format!("{author}, "));
    }
    let container = item.field("container-title");
    let link = doi_or_url(&item, "url");
    match kind {
        Kind::Article => note_article(&mut sb, &item, &title, &year, locator, label, terms),
        Kind::Chapter | Kind::Paper => {
            note_chapter(
                &mut sb,
                &item,
                &title,
                &year,
                locator,
                label,
                (terms, locale),
                is_editor,
            );
        }
        Kind::Webpage => {
            if !title.is_empty() {
                append_quoted(&mut sb, &title, ",", inside, "\"");
            }
            if let Some(c) = &container {
                sb.push(&format!(" {c}"));
            }
            sb.push(&format!(", {year}"));
            if let Some(locator) = locator {
                sb.push(&format!(
                    ", {}",
                    format_locator(locator, label, true, terms)
                ));
            }
            if let Some(link) = link.filter(|l| !l.is_empty()) {
                sb.push(&format!(", {link}"));
            }
        }
        Kind::Thesis => {
            if !title.is_empty() {
                append_quoted(&mut sb, &title, "", inside, "\"");
            }
            let short = terms.get("thesisPhdShort");
            let publisher = item.field("publisher");
            let parts = join_present(&[Some(&short), publisher.as_deref(), Some(&year)], ", ");
            if !parts.is_empty() {
                sb.push(&format!(" ({parts})"));
            }
            if let Some(locator) = locator {
                sb.push(&format!(
                    ", {}",
                    format_locator(locator, label, true, terms)
                ));
            }
        }
        Kind::Book | Kind::Report => {
            if !title.is_empty() {
                sb.push_italic(&title);
            }
            if let Some(edition) =
                format_edition(item.field("edition").as_deref(), terms, locale, false)
            {
                sb.push(&format!(", {edition}"));
            }
            let publisher = place_publisher(&item, "tight");
            let paren = join_present(&[publisher.as_deref(), Some(&year)], ", ");
            if !paren.is_empty() {
                sb.push(&format!(" ({paren})"));
            }
            if let Some(locator) = locator {
                sb.push(&format!(
                    ", {}",
                    format_locator(locator, label, true, terms)
                ));
            }
            if let Some(link) = link.filter(|l| !l.is_empty()) {
                sb.push(&format!(", {link}"));
            }
        }
    }
    push_suffix(&mut sb, entry.suffix.as_deref());
    sb.0
}

/// `formatCitation`: an in-text citation, or a footnote's text in the notes style.
pub fn format_citation(
    style: &str,
    items: &[CitationInput],
    context: &CitationContext,
) -> Formatted {
    if items.is_empty() {
        return Formatted::default();
    }
    let template = styles::template(style);
    let terms = styles::terms(&context.locale);
    let locale = context.locale.as_str();
    let separator = template.in_text_s("itemSeparator");
    let mode = template.in_text_s("mode");
    if mode == "note" {
        let mut sb = Spans::default();
        for (i, entry) in items.iter().enumerate() {
            if i > 0 {
                sb.push(&separator);
            }
            let spans = chicago_note_item(entry, &template, &terms, locale);
            sb.push_spans(&spans);
        }
        sb.ensure_period();
        return sb.build();
    }
    let item_id = |entry: &CitationInput| {
        entry
            .item
            .get("id")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    if mode == "numeric" {
        let base = context.index.unwrap_or(1.0);
        let mut sb = Spans::default();
        sb.push(&template.in_text_s("openDelimiter"));
        for (i, entry) in items.iter().enumerate() {
            if i > 0 {
                sb.push(&separator);
            }
            push_prefix(&mut sb, entry.prefix.as_deref());
            let number = context
                .indices
                .as_ref()
                .and_then(|m| m.get(&item_id(entry)).copied())
                .unwrap_or(base + i as f64);
            sb.push(&number_to_string(number));
            if let Some(locator) = entry.locator.as_deref().filter(|l| !l.is_empty()) {
                let label = entry.label.as_deref().unwrap_or("page");
                let loc =
                    format_locator(locator, label, template.in_text_b("omitPageLabel"), &terms);
                if !loc.is_empty() {
                    sb.push(&format!("{}{loc}", template.in_text_s("locatorPrefix")));
                }
            }
            push_suffix(&mut sb, entry.suffix.as_deref());
        }
        sb.push(&template.in_text_s("closeDelimiter"));
        return sb.build();
    }
    let suffixes = match (&context.year_suffixes, &context.library) {
        (Some(s), _) => s.clone(),
        (None, Some(library)) => compute_disambiguation(style, library, locale),
        (None, None) => {
            let list: Vec<JsValue> = items.iter().map(|e| e.item.clone()).collect();
            compute_disambiguation(style, &list, locale)
        }
    };
    let mut sb = Spans::default();
    sb.push(&template.in_text_s("openDelimiter"));
    let empty = JsObject::new();
    for (i, entry) in items.iter().enumerate() {
        if i > 0 {
            sb.push(&separator);
        }
        let item = Item(entry.item.as_object().unwrap_or(&empty));
        let label = entry.label.as_deref().unwrap_or("page");
        let locator = entry.locator.as_deref().filter(|l| !l.is_empty());
        push_prefix(&mut sb, entry.prefix.as_deref());
        let (names, _) = primary_contributors(&item);
        let mut lead = false;
        if !entry.suppress_author {
            if !names.is_empty() {
                sb.push(&in_text_authors(&names, &template, &terms, locale));
                lead = true;
            } else if item.field("title").is_some() {
                in_text_title(&mut sb, &item, &template);
                lead = true;
            }
        }
        if mode == "author-year" {
            let year = format_year(&item, &terms, suffixes.get(&item.id()).map(String::as_str));
            if lead {
                sb.push(&format!(
                    "{}{year}",
                    template.in_text_s("authorYearSeparator")
                ));
            } else {
                sb.push(&year);
            }
        } else if mode == "author-page"
            && !lead
            && locator.is_none()
            && item.field("title").is_some()
        {
            in_text_title(&mut sb, &item, &template);
        }
        if let Some(locator) = locator {
            let loc = format_locator(locator, label, template.in_text_b("omitPageLabel"), &terms);
            if !loc.is_empty() {
                let needs_prefix = mode == "author-year" || lead;
                let prefix = if needs_prefix {
                    template.in_text_s("locatorPrefix")
                } else {
                    String::new()
                };
                sb.push(&format!("{prefix}{loc}"));
            }
        }
        push_suffix(&mut sb, entry.suffix.as_deref());
    }
    sb.push(&template.in_text_s("closeDelimiter"));
    sb.build()
}

/// How `appendPrimaryBlock` sets what leads an entry.
struct Lead<'a> {
    author_suffix: &'a str,
    title_italic: bool,
    title_quoted: bool,
}

/// `appendPrimaryBlock`: the authors (or editors), or the title in their place.
fn primary_block(
    sb: &mut Spans,
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    lead: Lead,
) -> bool {
    let Lead {
        author_suffix,
        title_italic,
        title_quoted,
    } = lead;
    let (names, is_editor) = primary_contributors(item);
    if !names.is_empty() {
        let formatted = bib_name_list(&names, template, terms, locale, is_editor);
        sb.push(&formatted);
        if !author_suffix.is_empty() && !formatted.ends_with(author_suffix) {
            sb.push(author_suffix);
        }
        return false;
    }
    let Some(title) = item.field("title") else {
        return false;
    };
    if title_italic {
        sb.push_italic(&title);
        sb.push(author_suffix);
    } else if title_quoted {
        append_quoted(sb, &title, ".", terms.b("quotePunctuationInside"), "\"");
    } else {
        sb.push(&title);
        sb.push(author_suffix);
    }
    true
}

fn apa_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    year_suffix: Option<&str>,
) -> Spans {
    let mut sb = Spans::default();
    let kind = item.kind();
    let year = format_year(item, terms, year_suffix);
    let title_italic = is_standalone_italic(kind);
    let used_title = primary_block(
        &mut sb,
        item,
        template,
        terms,
        locale,
        Lead {
            author_suffix: ".",
            title_italic,
            title_quoted: false,
        },
    );
    if !sb.is_empty() {
        sb.push(" ");
    }
    sb.push(&format!("({year})."));
    let title = item.title();
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    if !used_title && let Some(title) = &title {
        sb.push(" ");
        if title_italic {
            sb.push_italic(title);
            if let Some(e) = &edition {
                sb.push(&format!(" ({e})"));
            }
            if kind == Kind::Thesis {
                let phd = terms.get("thesisPhd");
                let publisher = item.field("publisher");
                sb.push(&format!(
                    " [{}]",
                    join_present(&[Some(&phd), publisher.as_deref()], ", ")
                ));
            }
            sb.ensure_period();
        } else {
            sb.push(title);
            sb.ensure_period();
        }
    } else if used_title
        && title_italic
        && let Some(e) = &edition
    {
        sb.push(&format!(" ({e})."));
    }
    let container = item.field("container-title");
    let page = item.field("page");
    match kind {
        Kind::Article => {
            let mut parts = Spans::default();
            if let Some(c) = &container {
                parts.push_italic(c);
            }
            if let Some(v) = item.field("volume") {
                if !parts.is_empty() {
                    parts.push(", ");
                }
                parts.push_italic(&v);
                if let Some(i) = item.field("issue") {
                    parts.push(&format!("({i})"));
                }
            } else if let Some(i) = item.field("issue") {
                if !parts.is_empty() {
                    parts.push(", ");
                }
                parts.push(&format!("({i})"));
            }
            if let Some(p) = &page {
                if !parts.is_empty() {
                    parts.push(", ");
                }
                parts.push(&page_range(js_trim(p)));
            }
            if !parts.is_empty() {
                sb.push(" ");
                sb.push_spans(&parts.0);
                sb.ensure_period();
            }
        }
        Kind::Chapter | Kind::Paper => {
            let mut parts = Spans::default();
            parts.push(&format!("{} ", terms.get("inWord")));
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                parts.push(&format!(
                    "{}, ",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(c) = &container {
                parts.push_italic(c);
            }
            let page_text = page.as_ref().map(|p| page_label(p, terms));
            let bits = join_present(&[edition.as_deref(), page_text.as_deref()], ", ");
            if !bits.is_empty() {
                parts.push(&format!(" ({bits})"));
            }
            sb.push(" ");
            sb.push_spans(&parts.0);
            sb.ensure_period();
            if let Some(publisher) = place_publisher(item, "publisher-only") {
                sb.push(&format!(" {publisher}."));
            }
        }
        Kind::Webpage => {
            if let Some(c) = &container {
                sb.push(&format!(" {c}."));
            }
        }
        Kind::Thesis => {}
        Kind::Book | Kind::Report => {
            if let Some(publisher) = place_publisher(item, "publisher-only") {
                sb.push(&format!(" {publisher}."));
            }
        }
    }
    if let Some(link) = doi_or_url(item, "url").filter(|l| !l.is_empty()) {
        sb.push(&format!(" {link}"));
    }
    sb
}

fn chicago_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    year_suffix: Option<&str>,
    author_date: bool,
) -> Spans {
    let mut sb = Spans::default();
    let kind = item.kind();
    let year = format_year(item, terms, if author_date { year_suffix } else { None });
    let title_italic = matches!(kind, Kind::Book | Kind::Report);
    let title = item.title();
    let inside = terms.b("quotePunctuationInside");
    let used_title = primary_block(
        &mut sb,
        item,
        template,
        terms,
        locale,
        Lead {
            author_suffix: ".",
            title_italic,
            title_quoted: !title_italic,
        },
    );
    if author_date {
        if !sb.is_empty() {
            sb.push(" ");
        }
        sb.push(&format!("{year}."));
    }
    if !used_title && let Some(title) = &title {
        sb.push(" ");
        if title_italic {
            sb.push_italic(title);
            sb.ensure_period();
        } else {
            append_quoted(&mut sb, title, ".", inside, "\"");
        }
    }
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    let container = item.field("container-title");
    let page = item.field("page");
    let volume = item.field("volume");
    let issue = item.field("issue");
    match kind {
        Kind::Article => {
            if let Some(c) = &container {
                sb.push(" ");
                sb.push_italic(c);
            }
            if author_date {
                let vol_issue = match (&volume, &issue) {
                    (Some(v), Some(i)) => format!("{v} ({i})"),
                    (Some(v), None) => v.clone(),
                    (None, Some(i)) => format!("({i})"),
                    (None, None) => String::new(),
                };
                if !vol_issue.is_empty() {
                    sb.push(&format!(" {vol_issue}"));
                }
                if let Some(p) = &page {
                    sb.push(&format!(": {}", page_range(js_trim(p))));
                }
                sb.ensure_period();
            } else {
                let issue_text = issue
                    .as_ref()
                    .map(|i| format!("{} {i}", terms.get("issueWord")));
                let vol_issue = join_present(&[volume.as_deref(), issue_text.as_deref()], ", ");
                if !vol_issue.is_empty() {
                    sb.push(&format!(" {vol_issue}"));
                }
                sb.push(&format!(" ({year})"));
                if let Some(p) = &page {
                    sb.push(&format!(": {}", page_range(js_trim(p))));
                }
                sb.ensure_period();
            }
        }
        Kind::Chapter | Kind::Paper => {
            if let Some(c) = &container {
                sb.push(&format!(" {} ", terms.get("inWord")));
                sb.push_italic(c);
            }
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                sb.push(&format!(
                    ", {}",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(p) = &page {
                sb.push(&format!(", {}", page_range(js_trim(p))));
            }
            sb.ensure_period();
            let publisher = place_publisher(item, "tight");
            if author_date {
                if let Some(p) = publisher {
                    sb.push(&format!(" {p}."));
                }
            } else {
                let pub_year = join_present(&[publisher.as_deref(), Some(&year)], ", ");
                if !pub_year.is_empty() {
                    sb.push(&format!(" {pub_year}."));
                }
            }
        }
        Kind::Webpage => {
            let site = join_present(
                &[
                    container.as_deref(),
                    if author_date { None } else { Some(&year) },
                ],
                ", ",
            );
            if !site.is_empty() {
                sb.push(&format!(" {site}."));
            }
        }
        Kind::Thesis => {
            let short = terms.get("thesisPhdShort");
            let publisher = item.field("publisher");
            let parts = join_present(
                &[
                    Some(&short),
                    publisher.as_deref(),
                    if author_date { None } else { Some(&year) },
                ],
                ", ",
            );
            if !parts.is_empty() {
                sb.push(&format!(" {parts}."));
            }
        }
        Kind::Book | Kind::Report => {
            if let Some(e) = &edition {
                sb.push(&format!(" {e}"));
                sb.ensure_period();
            }
            let publisher = place_publisher(item, "tight");
            if author_date {
                if let Some(p) = publisher {
                    sb.push(&format!(" {p}."));
                }
            } else {
                let pub_year = join_present(&[publisher.as_deref(), Some(&year)], ", ");
                if !pub_year.is_empty() {
                    sb.push(&format!(" {pub_year}."));
                }
            }
        }
    }
    if let Some(link) = doi_or_url(item, "url").filter(|l| !l.is_empty()) {
        sb.push(&format!(" {link}."));
    }
    sb
}

fn mla_entry(item: &Item, template: &Template, terms: &styles::Terms, locale: &str) -> Spans {
    let mut sb = Spans::default();
    let kind = item.kind();
    let year = extract_year(item);
    let title_italic = is_standalone_italic(kind) && kind != Kind::Webpage;
    let title = item.title();
    let inside = terms.b("quotePunctuationInside");
    let used_title = primary_block(
        &mut sb,
        item,
        template,
        terms,
        locale,
        Lead {
            author_suffix: ".",
            title_italic,
            title_quoted: !title_italic,
        },
    );
    if !used_title && let Some(title) = &title {
        if !sb.is_empty() {
            sb.push(" ");
        }
        if title_italic {
            sb.push_italic(title);
            sb.ensure_period();
        } else {
            append_quoted(&mut sb, title, ".", inside, "\"");
        }
    }
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    let link = doi_or_url(item, "url").filter(|l| !l.is_empty());
    let container = item.field("container-title");
    let page = item.field("page");
    let close_tail = |sb: &mut Spans, tail: Spans| {
        if !tail.is_empty() {
            if !sb.is_empty() {
                sb.push(" ");
            }
            sb.push_spans(&tail.0);
            sb.ensure_period();
        }
    };
    match kind {
        Kind::Article => {
            let mut tail = Spans::default();
            if let Some(c) = &container {
                tail.push_italic(c);
            }
            if let Some(v) = item.field("volume") {
                tail.push(&format!(", {} {v}", terms.get("volumeWord")));
            }
            if let Some(i) = item.field("issue") {
                tail.push(&format!(", {} {i}", terms.get("issueWord")));
            }
            if let Some(y) = &year {
                tail.push(&format!(", {y}"));
            }
            if let Some(p) = &page {
                tail.push(&format!(", {}", page_label(p, terms)));
            }
            if let Some(l) = &link {
                tail.push(&format!(", {l}"));
            }
            close_tail(&mut sb, tail);
        }
        Kind::Chapter | Kind::Paper => {
            let mut tail = Spans::default();
            if let Some(c) = &container {
                tail.push_italic(c);
            }
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                tail.push(&format!(
                    ", {}",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(e) = &edition {
                tail.push(&format!(", {e}"));
            }
            if let Some(p) = item.field("publisher") {
                tail.push(&format!(", {p}"));
            }
            if let Some(y) = &year {
                tail.push(&format!(", {y}"));
            }
            if let Some(p) = &page {
                tail.push(&format!(", {}", page_label(p, terms)));
            }
            if let Some(l) = &link {
                tail.push(&format!(", {l}"));
            }
            close_tail(&mut sb, tail);
        }
        Kind::Webpage => {
            let mut tail = Spans::default();
            if let Some(c) = &container {
                tail.push_italic(c);
            }
            if let Some(y) = &year {
                let text = if tail.is_empty() {
                    y.clone()
                } else {
                    format!(", {y}")
                };
                tail.push(&text);
            }
            if let Some(l) = &link {
                let text = if tail.is_empty() {
                    l.clone()
                } else {
                    format!(", {l}")
                };
                tail.push(&text);
            }
            close_tail(&mut sb, tail);
        }
        Kind::Thesis => {
            let phd = terms.get("thesisPhd");
            let publisher = item.field("publisher");
            let second = join_present(&[publisher.as_deref(), Some(&phd)], ", ");
            let parts = join_present(&[year.as_deref(), Some(&second)], ". ");
            if !parts.is_empty() {
                sb.push(&format!(" {parts}."));
            }
        }
        Kind::Book | Kind::Report => {
            let no_date = terms.get("noDate");
            let publisher = item.field("publisher");
            let parts = join_present(
                &[
                    edition.as_deref(),
                    publisher.as_deref(),
                    Some(year.as_deref().unwrap_or(&no_date)),
                    link.as_deref(),
                ],
                ", ",
            );
            if !parts.is_empty() {
                sb.push(&format!(" {parts}."));
            }
        }
    }
    sb
}

fn harvard_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    year_suffix: Option<&str>,
) -> Spans {
    let mut sb = Spans::default();
    let kind = item.kind();
    let year = format_year(item, terms, year_suffix);
    let title_italic = is_standalone_italic(kind);
    let title = item.title();
    let used_title = primary_block(
        &mut sb,
        item,
        template,
        terms,
        locale,
        Lead {
            author_suffix: "",
            title_italic,
            title_quoted: false,
        },
    );
    if !sb.is_empty() {
        sb.push(" ");
    }
    sb.push(&format!("({year})"));
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, true);
    if !used_title && let Some(title) = &title {
        sb.push(" ");
        if title_italic {
            sb.push_italic(title);
            sb.ensure_period();
        } else {
            append_quoted(&mut sb, title, ",", false, "'");
        }
    }
    let container = item.field("container-title");
    let page = item.field("page");
    match kind {
        Kind::Article => {
            let mut parts = Spans::default();
            if let Some(c) = &container {
                parts.push_italic(c);
            }
            let vol_issue = match (item.field("volume"), item.field("issue")) {
                (Some(v), Some(i)) => format!("{v}({i})"),
                (Some(v), None) => v,
                (None, Some(i)) => format!("({i})"),
                (None, None) => String::new(),
            };
            if !vol_issue.is_empty() {
                parts.push(&format!(", {vol_issue}"));
            }
            if let Some(p) = &page {
                parts.push(&format!(", {}", page_label(p, terms)));
            }
            if !parts.is_empty() {
                sb.push(" ");
                sb.push_spans(&parts.0);
                sb.ensure_period();
            }
        }
        Kind::Chapter | Kind::Paper => {
            sb.push(&format!(" {} ", terms.get("inWord").to_lowercase()));
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                sb.push(&format!(
                    "{} ",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(c) = &container {
                sb.push_italic(c);
                sb.ensure_period();
            }
            let publisher = place_publisher(item, "tight");
            let page_text = page.as_ref().map(|p| page_label(p, terms));
            let tail = join_present(&[publisher.as_deref(), page_text.as_deref()], ", ");
            if !tail.is_empty() {
                sb.push(&format!(" {tail}."));
            }
        }
        Kind::Webpage => {
            if let Some(c) = &container {
                sb.push(&format!(" {c}."));
            }
        }
        Kind::Thesis => {
            let phd = terms.get("thesisPhdHarvard");
            let publisher = item.field("publisher");
            let parts = join_present(&[Some(&phd), publisher.as_deref()], ". ");
            if !parts.is_empty() {
                sb.push(&format!(" {parts}."));
            }
        }
        Kind::Book | Kind::Report => {
            if let Some(e) = &edition {
                sb.push(&format!(" {e}"));
                sb.ensure_period();
            }
            if let Some(p) = place_publisher(item, "tight") {
                sb.push(&format!(" {p}."));
            }
        }
    }
    if let Some(link) = doi_or_url(item, "url").filter(|l| !l.is_empty()) {
        sb.push(&format!(" {} {link}.", terms.get("availableAt")));
    }
    sb
}

fn ieee_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    index: usize,
) -> Spans {
    let mut sb = Spans::default();
    sb.push(&format!("[{index}] "));
    let kind = item.kind();
    let year = format_year(item, terms, None);
    let title = item.title();
    let inside = terms.b("quotePunctuationInside");
    let (names, is_editor) = primary_contributors(item);
    if !names.is_empty() {
        sb.push(&bib_name_list(&names, template, terms, locale, is_editor));
        sb.push(", ");
    }
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    let doi = doi_or_url(item, "doi-lower").filter(|d| !d.is_empty());
    let has_doi = item.field("DOI").is_some();
    let url = item.field("URL");
    let container = item.field("container-title");
    let page = item.field("page");
    let editors = item.names("editor");
    match kind {
        Kind::Article => {
            if let Some(t) = &title {
                append_quoted(&mut sb, t, ",", inside, "\"");
                sb.push(" ");
            }
            if let Some(c) = &container {
                sb.push_italic(c);
            }
            if let Some(v) = item.field("volume") {
                sb.push(&format!(", {} {v}", terms.get("volumeWord")));
            }
            if let Some(i) = item.field("issue") {
                sb.push(&format!(", {} {i}", terms.get("issueWord")));
            }
            if let Some(p) = &page {
                sb.push(&format!(", {}", page_label(p, terms)));
            }
            sb.push(&format!(", {year}"));
            if let Some(d) = doi.as_ref().filter(|_| has_doi) {
                sb.push(&format!(", {d}"));
            }
            sb.ensure_period();
            if !has_doi && let Some(u) = &url {
                sb.push(&format!(" {} {u}", terms.get("onlineAvailable")));
            }
        }
        Kind::Chapter | Kind::Paper => {
            if let Some(t) = &title {
                append_quoted(&mut sb, t, ",", inside, "\"");
                sb.push(" ");
            }
            if let Some(c) = &container {
                sb.push(&format!("{} ", terms.get("inWord").to_lowercase()));
                sb.push_italic(c);
            }
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                sb.push(&format!(
                    ", {}",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(p) = place_publisher(item, "tight") {
                sb.ensure_period();
                sb.push(&format!(" {p}"));
            } else if !editors.is_empty() {
                sb.ensure_period();
            }
            sb.push(&format!(", {year}"));
            if let Some(p) = &page {
                sb.push(&format!(", {}", page_label(p, terms)));
            }
            if let Some(d) = doi.as_ref().filter(|_| has_doi) {
                sb.push(&format!(", {d}"));
            }
            sb.ensure_period();
        }
        Kind::Webpage => {
            if let Some(t) = &title {
                append_quoted(&mut sb, t, ",", inside, "\"");
            }
            if let Some(c) = &container {
                sb.push(&format!(" {c},"));
            }
            sb.push(&format!(" {year}."));
            if let Some(u) = &url {
                sb.push(&format!(" {} {u}", terms.get("onlineAvailable")));
            }
        }
        Kind::Thesis => {
            if let Some(t) = &title {
                append_quoted(&mut sb, t, ",", inside, "\"");
            }
            let phd = terms.get("thesisPhdIeee");
            let publisher = item.field("publisher");
            let place = item.field("publisher-place");
            let tail = join_present(
                &[
                    Some(&phd),
                    publisher.as_deref(),
                    place.as_deref(),
                    Some(&year),
                ],
                ", ",
            );
            if !tail.is_empty() {
                sb.push(&format!(" {tail}."));
            }
        }
        Kind::Book | Kind::Report => {
            if let Some(t) = &title {
                sb.push_italic(t);
            }
            if let Some(e) = &edition {
                sb.push(&format!(", {e}"));
            }
            if let Some(p) = place_publisher(item, "tight") {
                sb.ensure_period();
                sb.push(&format!(" {p}"));
            }
            sb.push(&format!(", {year}."));
            if let Some(d) = doi.as_ref().filter(|_| has_doi) {
                sb.push(&format!(" {d}."));
            } else if let Some(u) = &url {
                sb.push(&format!(" {} {u}", terms.get("onlineAvailable")));
            }
        }
    }
    sb
}

fn vancouver_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    index: usize,
) -> Spans {
    let mut sb = Spans::default();
    sb.push(&format!("{index}. "));
    let kind = item.kind();
    let year = format_year(item, terms, None);
    let title = item.title();
    let (names, is_editor) = primary_contributors(item);
    if !names.is_empty() {
        sb.push(&bib_name_list(&names, template, terms, locale, is_editor));
        sb.ensure_period();
        sb.push(" ");
    }
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    let doi = if item.field("DOI").is_some() {
        doi_or_url(item, "doi-lower").filter(|d| !d.is_empty())
    } else {
        None
    };
    let url = item.field("URL");
    let container = item.field("container-title");
    let page = item.field("page");
    match kind {
        Kind::Article => {
            if let Some(t) = &title {
                sb.push(t);
                sb.ensure_period();
                sb.push(" ");
            }
            if let Some(c) = &container {
                sb.push(&format!("{c}. "));
            }
            let mut meta = year.clone();
            if let Some(v) = item.field("volume") {
                meta.push_str(&format!(";{v}"));
                if let Some(i) = item.field("issue") {
                    meta.push_str(&format!("({i})"));
                }
            } else if let Some(i) = item.field("issue") {
                meta.push_str(&format!(";({i})"));
            }
            if let Some(p) = &page {
                meta.push_str(&format!(":{}", page_range(js_trim(p))));
            }
            sb.push(&format!("{meta}."));
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            } else if let Some(u) = &url {
                sb.push(&format!(" {} {u}", terms.get("availableFrom")));
            }
        }
        Kind::Chapter | Kind::Paper => {
            if let Some(t) = &title {
                sb.push(t);
                sb.ensure_period();
                sb.push(" ");
            }
            sb.push(&format!("{}: ", terms.get("inWord")));
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                sb.push(&format!(
                    "{}. ",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(c) = &container {
                sb.push(&format!("{c}. "));
            }
            if let Some(p) = place_publisher(item, "tight") {
                sb.push(&format!("{p}; "));
            }
            sb.push(&format!("{year}."));
            if let Some(p) = &page {
                sb.push(&format!(
                    " {} {}.",
                    terms.get("pageSingle"),
                    page_range(js_trim(p))
                ));
            }
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            }
        }
        Kind::Webpage => {
            if let Some(t) = &title {
                sb.push(&format!("{t} [Internet]. "));
            }
            if let Some(c) = &container {
                sb.push(&format!("{c}; "));
            }
            sb.push(&format!("{year}."));
            if let Some(u) = &url {
                sb.push(&format!(" {} {u}", terms.get("availableFrom")));
            }
        }
        Kind::Thesis => {
            if let Some(t) = &title {
                sb.push(&format!("{t} {}. ", terms.get("thesisVancouver")));
            }
            if let Some(p) = item.field("publisher") {
                sb.push(&format!("{p}; "));
            }
            sb.push(&format!("{year}."));
        }
        Kind::Book | Kind::Report => {
            if let Some(t) = &title {
                sb.push(t);
                sb.ensure_period();
            }
            if let Some(e) = &edition {
                sb.push(&format!(" {e}"));
                sb.ensure_period();
            }
            match place_publisher(item, "tight") {
                Some(p) => sb.push(&format!(" {p}; {year}.")),
                None => sb.push(&format!(" {year}.")),
            };
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            }
        }
    }
    sb
}

fn din_entry(
    item: &Item,
    template: &Template,
    terms: &styles::Terms,
    locale: &str,
    year_suffix: Option<&str>,
) -> Spans {
    let mut sb = Spans::default();
    let kind = item.kind();
    let year = format_year(item, terms, year_suffix);
    let title_italic = is_standalone_italic(kind);
    let title = item.title();
    let used_title = primary_block(
        &mut sb,
        item,
        template,
        terms,
        locale,
        Lead {
            author_suffix: ":",
            title_italic,
            title_quoted: false,
        },
    );
    if !used_title && let Some(t) = &title {
        if !sb.is_empty() {
            sb.push(" ");
        }
        if title_italic {
            sb.push_italic(t);
        } else {
            sb.push(t);
        }
        sb.ensure_period();
    }
    let edition = format_edition(item.field("edition").as_deref(), terms, locale, false);
    let doi = if item.field("DOI").is_some() {
        doi_or_url(item, "doi-upper").filter(|d| !d.is_empty())
    } else {
        None
    };
    let url = item.field("URL");
    let container = item.field("container-title");
    let page = item.field("page");
    match kind {
        Kind::Article => {
            sb.push(&format!(" {}: ", terms.get("inWord")));
            if let Some(c) = &container {
                sb.push_italic(c);
            }
            if let Some(v) = item.field("volume") {
                sb.push(&format!(" {} {v}", terms.get("volumeCap")));
            }
            sb.push(&format!(" ({year})"));
            if let Some(i) = item.field("issue") {
                sb.push(&format!(", {} {i}", terms.get("issueCap")));
            }
            if let Some(p) = &page {
                sb.push(&format!(", {}", page_label(p, terms)));
            }
            sb.ensure_period();
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            } else if let Some(u) = &url {
                sb.push(&format!(" URL: {u}."));
            }
        }
        Kind::Chapter | Kind::Paper => {
            sb.push(&format!(" {}: ", terms.get("inWord")));
            let editors = item.names("editor");
            if !editors.is_empty() {
                let mode = template.names_s("containerEditorMode");
                sb.push(&format!(
                    "{}: ",
                    container_editors(&editors, &mode, terms, locale)
                ));
            }
            if let Some(c) = &container {
                sb.push_italic(c);
                sb.ensure_period();
            }
            if let Some(e) = &edition {
                sb.push(&format!(" {e}"));
                sb.ensure_period();
            }
            let publisher = place_publisher(item, "spaced");
            let pub_year = join_present(&[publisher.as_deref(), Some(&year)], ", ");
            let page_text = page.as_ref().map(|p| page_label(p, terms));
            let tail = join_present(&[Some(&pub_year), page_text.as_deref()], ", ");
            if !tail.is_empty() {
                sb.push(&format!(" {tail}."));
            }
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            }
        }
        Kind::Webpage => {
            if let Some(c) = &container {
                sb.push(&format!(" {c},"));
            }
            if let Some(u) = &url {
                sb.push(&format!(" URL: {u}"));
            }
            sb.push(&format!(" ({year})."));
        }
        Kind::Thesis => {
            let din = terms.get("thesisDin");
            let publisher = item.field("publisher");
            let parts = join_present(&[publisher.as_deref(), Some(&din), Some(&year)], ", ");
            if !parts.is_empty() {
                sb.push(&format!(" {parts}."));
            }
        }
        Kind::Book | Kind::Report => {
            if let Some(e) = &edition {
                sb.push(&format!(" {e}"));
                sb.ensure_period();
            }
            let publisher = place_publisher(item, "spaced");
            let pub_year = join_present(&[publisher.as_deref(), Some(&year)], ", ");
            if !pub_year.is_empty() {
                sb.push(&format!(" {pub_year}."));
            }
            if let Some(d) = &doi {
                sb.push(&format!(" {d}."));
            } else if let Some(u) = &url {
                sb.push(&format!(" URL: {u}."));
            }
        }
    }
    sb
}

/// `formatBibliography`: the sources as a reference list, sorted and disambiguated as the
/// style wants.
pub fn format_bibliography(style: &str, items: &[JsValue], locale: &str) -> Vec<BibliographyEntry> {
    if items.is_empty() {
        return Vec::new();
    }
    let template = styles::template(style);
    let terms = styles::terms(locale);
    let sorted = sort_items(items, &template, &terms, locale);
    let sorted_values: Vec<JsValue> = sorted
        .iter()
        .map(|o| JsValue::Object((*o).clone()))
        .collect();
    let disambiguation = compute_disambiguation(style, &sorted_values, locale);
    sorted
        .iter()
        .enumerate()
        .map(|(i, object)| {
            let item = Item(object);
            let index = i + 1;
            let suffix = disambiguation.get(&item.id()).map(String::as_str);
            let spans = match template.bib_s("layout").as_str() {
                "apa" => apa_entry(&item, &template, &terms, locale, suffix),
                "chicago-ad" => chicago_entry(&item, &template, &terms, locale, suffix, true),
                "chicago-nb" => chicago_entry(&item, &template, &terms, locale, suffix, false),
                "mla" => mla_entry(&item, &template, &terms, locale),
                "harvard" => harvard_entry(&item, &template, &terms, locale, suffix),
                "ieee" => ieee_entry(&item, &template, &terms, locale, index),
                "vancouver" => vancouver_entry(&item, &template, &terms, locale, index),
                _ => din_entry(&item, &template, &terms, locale, suffix),
            };
            let mut sb = Spans::default();
            sb.push_spans(&spans.0);
            let built = sb.build();
            BibliographyEntry {
                id: item.id(),
                text: built.text,
                html: built.html,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn doi_prefixes_are_matched_as_javascript_matches_them() {
        assert_eq!(super::normalize_doi("HTTPS://DX.DOI.ORG/10.1/x"), "10.1/x");
        assert_eq!(super::normalize_doi("Doi: 10.1/x"), "10.1/x");
        // `/^https?:…/i` and `/^doi:/i` don't take "ſ" for "s" or "ı" for "i" in JavaScript.
        assert_eq!(
            super::normalize_doi("http\u{17f}://doi.org/10.1/x"),
            "http\u{17f}://doi.org/10.1/x"
        );
        assert_eq!(
            super::normalize_doi("do\u{131}: 10.1/x"),
            "do\u{131}: 10.1/x"
        );
    }
}
