//! A project's citation library as CSL-JSON (ProseCraft `src/lib/citations/csl.ts`, upstream
//! `7dae544`): items checked, cleaned and capped as ProseCraft keeps them, each with a unique
//! id (a stable one made from the item when it has none).

use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsString, JsValue, number_to_string, stringify};
use std::collections::HashSet;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

pub const MAX_CITATION_ITEMS: usize = 5000;
pub const MAX_CITATION_ID_LENGTH: usize = 128;
pub const MAX_CITATION_TYPE_LENGTH: usize = 64;
pub const MAX_CITATION_TITLE_LENGTH: usize = 2000;
pub const MAX_CITATION_FIELD_LENGTH: usize = 1000;
pub const MAX_CITATION_SHORT_FIELD_LENGTH: usize = 128;
pub const MAX_CITATION_URL_LENGTH: usize = 2000;
pub const MAX_CITATION_ABSTRACT_LENGTH: usize = 20000;
pub const MAX_CITATION_NAME_PART_LENGTH: usize = 300;
pub const MAX_CITATION_CONTRIBUTORS: usize = 200;

macro_rules! rule {
    ($name:ident, $source:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, ""))
        }
    };
}

rule!(csl_type, r"^[A-Za-z][A-Za-z0-9_-]{0,63}$");
rule!(
    control_chars,
    r"[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]"
);
rule!(id_disallowed, r"[^A-Za-z0-9_.:/-]+");
rule!(dashes, r"-{2,}");
rule!(id_ends, r"^[-./]+|[-./]+$");
rule!(combining, r"[̀-ͯ]");
rule!(not_slug, r"[^a-z0-9]+");
rule!(whitespace_run, r"\s+");

fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn is_space_unit(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(is_js_whitespace)
}

/// `.trim()` on UTF-16 units.
fn trim_units(units: &[u16]) -> &[u16] {
    let start = units.iter().take_while(|u| is_space_unit(**u)).count();
    let end = units.len()
        - units[start..]
            .iter()
            .rev()
            .take_while(|u| is_space_unit(**u))
            .count();
    &units[start..end]
}

/// `text.slice(0, max).trim()`, by UTF-16 units as JavaScript cuts it.
fn cut_units(units: &[u16], max: usize) -> JsString {
    JsString::from_units(trim_units(&units[..units.len().min(max)]).to_vec())
}

/// `/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/`.
fn is_control_unit(unit: u16) -> bool {
    matches!(unit, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)
}

/// Runs of units matching `class` (at least `min` long) replaced by `with`, as
/// `replace(/[class]{min,}/g, with)` does.
fn squeeze_units(units: &[u16], class: impl Fn(u16) -> bool, min: usize, with: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let run = units[i..].iter().take_while(|u| class(**u)).count();
        if run >= min.max(1) {
            out.extend_from_slice(with);
            i += run;
        } else {
            out.push(units[i]);
            i += 1;
        }
    }
    out
}

/// The cleaning works on UTF-16 units, as the TypeScript does, so a lone surrogate in a
/// CSL-JSON string (`"\ud83d"`) is kept rather than replaced.
fn units_of(value: Option<&JsValue>) -> Option<Vec<u16>> {
    value.and_then(JsValue::as_js_string).map(|s| {
        s.units()
            .iter()
            .copied()
            .filter(|u| !is_control_unit(*u))
            .collect()
    })
}

/// `cleanSingleLine`: one line, control characters removed, runs of space as one.
fn clean_single_line(value: Option<&JsValue>, max: usize) -> Option<JsString> {
    let units = units_of(value)?;
    let line_break = |u: u16| matches!(u, 0x0d | 0x0a | 0x09);
    let units = squeeze_units(&units, line_break, 1, &[0x20]);
    let units = squeeze_units(&units, is_space_unit, 2, &[0x20]);
    let cleaned = trim_units(&units);
    (!cleaned.is_empty()).then(|| cut_units(cleaned, max))
}

/// `cleanMultiLine`: control characters removed and line endings as `\n`.
fn clean_multi_line(value: Option<&JsValue>, max: usize) -> Option<JsString> {
    let units = units_of(value)?;
    let mut lines = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        if units[i] == 0x0d {
            lines.push(0x0a);
            i += if units.get(i + 1) == Some(&0x0a) {
                2
            } else {
                1
            };
        } else {
            lines.push(units[i]);
            i += 1;
        }
    }
    let cleaned = trim_units(&lines);
    (!cleaned.is_empty()).then(|| cut_units(cleaned, max))
}

/// `cleanStringOrNumber`.
fn clean_string_or_number(value: Option<&JsValue>, max: usize) -> Option<JsString> {
    if let Some(n) = value.and_then(JsValue::as_number) {
        if !n.is_finite() {
            return None;
        }
        let text = number_to_string(n);
        return Some(JsString::from_units(
            text.encode_utf16().take(max).collect(),
        ));
    }
    clean_single_line(value, max)
}

/// `cleanUrl`: an absolute http, https or ftp address, kept as written.
fn clean_url(value: Option<&JsValue>) -> Option<JsString> {
    let raw = clean_single_line(value, MAX_CITATION_URL_LENGTH)?;
    let parsed = url::Url::parse(&raw.to_string_lossy()).ok()?;
    ["http", "https", "ftp"]
        .contains(&parsed.scheme())
        .then_some(raw)
}

/// `sanitizeCitationId`: a citation id as a safe, bounded token.
pub fn sanitize_citation_id(raw: &str) -> String {
    let cleaned = control_chars().replace(raw, true, "");
    let cleaned = js_trim(&cleaned);
    let cleaned = id_disallowed().replace(cleaned, true, "-");
    let cleaned = dashes().replace(&cleaned, true, "-");
    let cleaned = id_ends().replace(&cleaned, true, "");
    // Only ASCII is left, so characters are UTF-16 units.
    cleaned.chars().take(MAX_CITATION_ID_LENGTH).collect()
}

/// `fnv1aHex`: FNV-1a over UTF-16 units, in base 36, at least six digits.
fn fnv1a_hex(input: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in input.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut digits = Vec::new();
    let mut n = hash;
    loop {
        let d = (n % 36) as u8;
        digits.push(if d < 10 { b'0' + d } else { b'a' + d - 10 });
        n /= 36;
        if n == 0 {
            break;
        }
    }
    while digits.len() < 6 {
        digits.push(b'0');
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

/// `slugPart`: ASCII letters and digits, accents dropped, at most 24.
fn slug_part(text: Option<&str>) -> String {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return String::new();
    };
    let decomposed: String = text.nfkd().collect();
    let plain = combining().replace(&decomposed, true, "");
    let lower = plain.to_lowercase();
    let slug = not_slug().replace(&lower, true, "");
    slug.chars().take(24).collect()
}

fn get_text(object: &JsObject, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
}

/// `buildStableFallbackId`: from the first author or title and the year, and a fingerprint.
fn stable_fallback_id(item: &JsObject) -> String {
    let first_name = |key: &str| {
        item.get(key)
            .and_then(JsValue::as_array)
            .and_then(|list| list.first())
            .and_then(JsValue::as_object)
            .cloned()
    };
    let first = first_name("author").or_else(|| first_name("editor"));
    let author = first
        .as_ref()
        .and_then(|n| get_text(n, "family").or_else(|| get_text(n, "literal")));
    let author_slug = slug_part(author.as_deref());
    let first_year = item
        .get("issued")
        .and_then(|i| i.get("date-parts"))
        .and_then(JsValue::as_array)
        .and_then(|parts| parts.first())
        .and_then(JsValue::as_array)
        .and_then(|group| group.first());
    let year_slug = match first_year {
        Some(JsValue::Number(n)) => slug_part(Some(&number_to_string(*n))),
        Some(JsValue::String(s)) => slug_part(Some(&s.to_string_lossy())),
        _ => String::new(),
    };
    let title = get_text(item, "title");
    // `title.split(/\s+/)[0]`.
    let first_word = title
        .as_deref()
        .map(|t| t[..whitespace_run().search(t).unwrap_or(t.len())].to_owned());
    let title_slug = slug_part(first_word.as_deref());
    let or = |key: &str, default: JsValue| item.get(key).cloned().unwrap_or(default);
    let fingerprint = fnv1a_hex(&stringify(&JsValue::Array(vec![
        or("type", JsValue::Null),
        or("title", JsValue::str("")),
        or("author", JsValue::Array(Vec::new())),
        or("editor", JsValue::Array(Vec::new())),
        or("issued", JsValue::Null),
        or("container-title", JsValue::str("")),
        or("DOI", JsValue::str("")),
        or("URL", JsValue::str("")),
        or("ISBN", JsValue::str("")),
    ])));
    let lead = [&author_slug, &title_slug]
        .into_iter()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "ref".into());
    let prefix = if year_slug.is_empty() {
        lead
    } else {
        format!("{lead}-{year_slug}")
    };
    format!("{prefix}-{fingerprint}")
        .chars()
        .take(MAX_CITATION_ID_LENGTH)
        .collect()
}

/// `normalizeName`.
fn normalize_name(value: &JsValue) -> Option<JsValue> {
    let object = value.as_object()?;
    let part = |key: &str| clean_single_line(object.get(key), MAX_CITATION_NAME_PART_LENGTH);
    let fields = [
        ("family", part("family")),
        ("given", part("given")),
        ("literal", part("literal")),
        ("non-dropping-particle", part("non-dropping-particle")),
        ("dropping-particle", part("dropping-particle")),
        ("suffix", part("suffix")),
    ];
    if fields[..3].iter().all(|(_, v)| v.is_none()) {
        return None;
    }
    let mut name = JsObject::new();
    for (key, value) in fields {
        if let Some(value) = value {
            name.insert(key, JsValue::String(value));
        }
    }
    Some(JsValue::Object(name))
}

/// `normalizeNameList`: at most 200 names; none when none is left.
fn normalize_name_list(value: Option<&JsValue>) -> Option<JsValue> {
    let list = value?.as_array()?;
    let mut names = Vec::new();
    for entry in list {
        if names.len() >= MAX_CITATION_CONTRIBUTORS {
            break;
        }
        if let Some(name) = normalize_name(entry) {
            names.push(name);
        }
    }
    (!names.is_empty()).then_some(JsValue::Array(names))
}

/// `normalizeDateParts`: two groups at most, of three parts at most (whole numbers, or short
/// strings).
fn normalize_date_parts(value: Option<&JsValue>) -> Option<JsValue> {
    let groups = value?.as_array()?;
    let mut parts = Vec::new();
    for group in groups.iter().take(2) {
        let Some(group) = group.as_array() else {
            continue;
        };
        let mut cleaned = Vec::new();
        for part in group.iter().take(3) {
            match part {
                JsValue::Number(n) if n.is_finite() && n.fract() == 0.0 => {
                    cleaned.push(JsValue::Number(*n));
                }
                JsValue::String(_) => {
                    if let Some(text) = clean_single_line(Some(part), 16) {
                        cleaned.push(JsValue::String(text));
                    }
                }
                _ => {}
            }
        }
        if !cleaned.is_empty() {
            parts.push(JsValue::Array(cleaned));
        }
    }
    (!parts.is_empty()).then_some(JsValue::Array(parts))
}

/// `normalizeDate`.
fn normalize_date(value: Option<&JsValue>) -> Option<JsValue> {
    let object = value?.as_object()?;
    let date_parts = normalize_date_parts(object.get("date-parts"));
    let literal = clean_single_line(object.get("literal"), MAX_CITATION_SHORT_FIELD_LENGTH);
    let raw = clean_single_line(object.get("raw"), MAX_CITATION_SHORT_FIELD_LENGTH);
    if date_parts.is_none() && literal.is_none() && raw.is_none() {
        return None;
    }
    let mut date = JsObject::new();
    if let Some(parts) = date_parts {
        date.insert("date-parts", parts);
    }
    if let Some(literal) = literal {
        date.insert("literal", JsValue::String(literal));
    }
    if let Some(raw) = raw {
        date.insert("raw", JsValue::String(raw));
    }
    match object.get("season") {
        Some(JsValue::Number(n)) if n.is_finite() && n.fract() == 0.0 => {
            date.insert("season", JsValue::Number(*n));
        }
        season => {
            if let Some(season) = clean_single_line(season, 32) {
                date.insert("season", JsValue::String(season));
            }
        }
    }
    if let Some(JsValue::Bool(circa)) = object.get("circa") {
        date.insert("circa", JsValue::Bool(*circa));
    }
    Some(JsValue::Object(date))
}

/// `allocateUniqueId`: the id, or it with `-2`, `-3`… when it is taken.
fn allocate_unique_id(base: &str, seen: &mut HashSet<String>) -> (String, bool) {
    if seen.insert(base.to_owned()) {
        return (base.to_owned(), false);
    }
    let base_units: Vec<u16> = base.encode_utf16().collect();
    for counter in 2..MAX_CITATION_ITEMS + 10 {
        let suffix = format!("-{counter}");
        let keep = MAX_CITATION_ID_LENGTH
            .saturating_sub(suffix.len())
            .min(base_units.len());
        let candidate = format!("{}{suffix}", String::from_utf16_lossy(&base_units[..keep]));
        if seen.insert(candidate.clone()) {
            return (candidate, true);
        }
    }
    let fallback = format!("ref-{}", seen.len() + 1);
    seen.insert(fallback.clone());
    (fallback, true)
}

/// `normalizeSingleItem`: the item's fields in ProseCraft's order, and the id it gave.
fn normalize_item(value: &JsValue) -> Result<(JsObject, Option<String>), &'static str> {
    let Some(object) = value.as_object() else {
        return Err("Citation entry is not an object.");
    };
    let raw_type = clean_single_line(object.get("type"), MAX_CITATION_TYPE_LENGTH)
        .map(|t| t.to_string_lossy())
        .filter(|t| csl_type().test(t))
        .ok_or("Citation entry is missing a valid CSL type.")?;
    let line = |key: &str, max: usize| clean_single_line(object.get(key), max);
    let either = |key: &str, max: usize| clean_string_or_number(object.get(key), max);
    let title = line("title", MAX_CITATION_TITLE_LENGTH);
    let author = normalize_name_list(object.get("author"));
    let editor = normalize_name_list(object.get("editor"));
    let issued = normalize_date(object.get("issued"));
    let container_title = line("container-title", MAX_CITATION_TITLE_LENGTH);
    let publisher = line("publisher", MAX_CITATION_FIELD_LENGTH);
    let publisher_place = line("publisher-place", MAX_CITATION_FIELD_LENGTH);
    let volume = either("volume", MAX_CITATION_SHORT_FIELD_LENGTH);
    let issue = either("issue", MAX_CITATION_SHORT_FIELD_LENGTH);
    let page = line("page", MAX_CITATION_SHORT_FIELD_LENGTH);
    let doi = line("DOI", 500);
    let url = clean_url(object.get("URL"));
    let isbn = line("ISBN", 64);
    let edition = either("edition", MAX_CITATION_SHORT_FIELD_LENGTH);
    let language = line("language", 64);
    let abstract_free = clean_multi_line(object.get("abstract-free"), MAX_CITATION_ABSTRACT_LENGTH)
        .or_else(|| clean_multi_line(object.get("abstract"), MAX_CITATION_ABSTRACT_LENGTH));
    let cite_key = line("citeKey", MAX_CITATION_ID_LENGTH);
    let provider = line("provider", 64);
    let has_content = title.is_some()
        || author.is_some()
        || editor.is_some()
        || container_title.is_some()
        || publisher.is_some()
        || doi.is_some()
        || url.is_some()
        || isbn.is_some()
        || issued.is_some()
        || abstract_free.is_some();
    if !has_content {
        return Err("Citation entry has no bibliographic content.");
    }
    let mut item = JsObject::new();
    item.insert("type", JsValue::str(&raw_type.to_lowercase()));
    let text = |v: Option<JsString>| v.map(JsValue::String);
    for (key, value) in [
        ("title", text(title)),
        ("author", author),
        ("editor", editor),
        ("issued", issued),
        ("container-title", text(container_title)),
        ("publisher", text(publisher)),
        ("publisher-place", text(publisher_place)),
        ("volume", text(volume)),
        ("issue", text(issue)),
        ("page", text(page)),
        ("DOI", text(doi)),
        ("URL", text(url)),
        ("ISBN", text(isbn)),
        ("edition", text(edition)),
        ("language", text(language)),
        ("abstract-free", text(abstract_free)),
        ("citeKey", text(cite_key)),
        ("provider", text(provider)),
    ] {
        if let Some(value) = value {
            item.insert(key, value);
        }
    }
    let raw_id = clean_string_or_number(object.get("id"), MAX_CITATION_ID_LENGTH)
        .map(|id| id.to_string_lossy());
    Ok((item, raw_id))
}

/// The items kept and why others were not.
#[derive(Clone, Debug, PartialEq)]
pub struct CslNormalization {
    pub items: Vec<JsValue>,
    pub warnings: Vec<String>,
}

/// `normalizeCslItemsDetailed`: the citation items of `value`, checked, cleaned, capped at
/// 5,000 and given unique ids, and a warning for each item left out or renamed.
pub fn normalize_csl_items_detailed(value: &JsValue) -> CslNormalization {
    let Some(entries) = value.as_array() else {
        return CslNormalization {
            items: Vec::new(),
            warnings: vec!["Expected an array of citation items.".into()],
        };
    };
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        if items.len() >= MAX_CITATION_ITEMS {
            warnings.push(format!(
                "Citation library capped at {MAX_CITATION_ITEMS} items; remaining entries were skipped."
            ));
            break;
        }
        let (item, raw_id) = match normalize_item(entry) {
            Ok(normalized) => normalized,
            Err(error) => {
                warnings.push(format!("Entry {}: {error}", index + 1));
                continue;
            }
        };
        let sanitized = raw_id
            .map(|id| sanitize_citation_id(&id))
            .unwrap_or_default();
        let base = if sanitized.is_empty() {
            stable_fallback_id(&item)
        } else {
            sanitized
        };
        let (id, renamed) = allocate_unique_id(&base, &mut seen);
        if renamed {
            warnings.push(format!(
                "Duplicate citation id \"{base}\" renamed to \"{id}\"."
            ));
        }
        let mut with_id = JsObject::new();
        with_id.insert("id", JsValue::str(&id));
        for (key, value) in item.iter() {
            with_id.insert(key.clone(), value.clone());
        }
        items.push(JsValue::Object(with_id));
    }
    CslNormalization { items, warnings }
}

/// `normalizeCslItems`.
pub fn normalize_csl_items(value: &JsValue) -> Vec<JsValue> {
    normalize_csl_items_detailed(value).items
}

const NAME_PARTICLES: [&str; 19] = [
    "al", "d", "da", "de", "del", "della", "den", "der", "di", "dos", "du", "el", "la", "le",
    "van", "von", "zu", "zum", "zur",
];

/// `cleanSingleLine` over a plain string.
fn clean_line(text: &str, max: usize) -> Option<JsString> {
    clean_single_line(Some(&JsValue::str(text)), max)
}

/// A name object from its parts, in ProseCraft's order (family, given, suffix), or `None`
/// when it has neither a family nor a given name.
fn name_of(
    family: Option<JsString>,
    given: Option<JsString>,
    suffix: Option<JsString>,
) -> Option<JsValue> {
    if family.is_none() && given.is_none() {
        return None;
    }
    let mut name = JsObject::new();
    for (key, value) in [("family", family), ("given", given), ("suffix", suffix)] {
        if let Some(value) = value {
            name.insert(key, JsValue::String(value));
        }
    }
    Some(JsValue::Object(name))
}

/// `parsePersonName`: one personal name ("Last, First", "Last, Jr., First" or "First Last")
/// as a CSL name object, or `None` when nothing is left of it.
pub fn parse_person_name(raw: &str) -> Option<JsValue> {
    let trimmed = clean_line(raw, MAX_CITATION_NAME_PART_LENGTH * 2)?.to_string_lossy();
    let part = |text: &str| clean_line(text, MAX_CITATION_NAME_PART_LENGTH);
    if trimmed.contains(',') {
        let parts: Vec<&str> = trimmed.split(',').map(js_trim).collect();
        if parts.len() >= 3 {
            let family = part(parts[0]);
            let suffix = part(parts[1]);
            let given = part(&parts[2..].join(", "));
            return name_of(family, given, suffix);
        }
        let family = parts.first().and_then(|p| part(p));
        let given = parts.get(1).and_then(|p| part(p));
        return name_of(family, given, None);
    }
    let words: Vec<&str> = trimmed
        .split(is_js_whitespace)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() == 1 {
        return name_of(part(words[0]), None, None);
    }
    let mut family_start = words.len().saturating_sub(1);
    while family_start > 1
        && NAME_PARTICLES.contains(&words[family_start - 1].to_lowercase().as_str())
    {
        family_start -= 1;
    }
    let given = part(&words[..family_start].join(" "));
    let family = part(&words[family_start..].join(" "));
    name_of(family, given, None)
}

/// `MONTH_NAMES`: English and German month names and abbreviations.
fn month_by_name(name: &str) -> Option<f64> {
    let month = match name {
        "jan" | "january" | "januar" => 1,
        "feb" | "february" | "februar" => 2,
        "mar" | "march" | "maerz" | "märz" => 3,
        "apr" | "april" => 4,
        "may" | "mai" => 5,
        "jun" | "june" | "juni" => 6,
        "jul" | "july" | "juli" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" | "oktober" => 10,
        "nov" | "november" => 11,
        "dec" | "december" | "dezember" => 12,
        _ => return None,
    };
    Some(f64::from(month))
}

/// `parseMonthNumber`: a month (1 to 12) from a number or an English or German month name.
/// (Names are looked up as own entries only, as since jennraeross/ProseCraft#48; before it,
/// `constructor` and `__proto__` found `Object.prototype` members.)
pub fn parse_month_number(raw: &str) -> Option<f64> {
    let lower = js_trim(raw).to_lowercase();
    let cleaned = lower.strip_suffix('.').unwrap_or(&lower);
    if cleaned.is_empty() {
        return None;
    }
    if (1..=2).contains(&cleaned.len()) && cleaned.bytes().all(|b| b.is_ascii_digit()) {
        let number = crate::js::string_to_number(cleaned);
        return (1.0..=12.0).contains(&number).then_some(number);
    }
    month_by_name(cleaned)
}
