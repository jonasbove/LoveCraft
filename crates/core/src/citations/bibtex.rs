//! BibTeX and BibLaTeX import (ProseCraft `src/lib/citations/bibtex.ts`, upstream `7dae544`):
//! entries read into CSL-JSON items, LaTeX accents and macros turned into Unicode text, and a
//! warning for each part that could not be read.

use crate::csl::{Inherited, parse_month_value, parse_person_name};
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsValue, string_to_number};
use std::collections::HashMap;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// Items read from a BibTeX file, and why parts of it were not.
#[derive(Clone, Debug, PartialEq)]
pub struct BibtexParse {
    pub items: Vec<JsValue>,
    pub warnings: Vec<String>,
}

macro_rules! rule {
    ($name:ident, $source:expr) => {
        rule!($name, $source, "");
    };
    ($name:ident, $source:expr, $flags:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

rule!(
    symbol_accent,
    r#"\{\\(["'`^~=.])\s*(?:\\([ij])\b|\{?([A-Za-z])\}?)\s*\}|\\(["'`^~=.])\s*(?:\{\s*(?:\\([ij])\b|([A-Za-z]))\s*\}|\\([ij])\b|([A-Za-z]))"#
);
rule!(
    letter_accent,
    r"\{\\([uvHckrdb])\s+(?:\\([ij])\b|\{?([A-Za-z])\}?)\s*\}|\{\\([uvHckrdb])\{\s*(?:\\([ij])\b|([A-Za-z]))\s*\}\}|\\([uvHckrdb])\{\s*(?:\\([ij])\b|([A-Za-z]))\s*\}|\\([uvHckrdb])\s+(?:\\([ij])\b|([A-Za-z]))\b"
);
rule!(
    named_macro,
    r"\\(textquotedblleft|textquotedblright|textquoteleft|textquoteright|textquotesingle|textasciitilde|textbackslash|textendash|textemdash|ldots|dots|SS|ss|AE|ae|OE|oe|AA|aa|DH|dh|TH|th|O|o|L|l|i|j)(?:\{\}|\s+|(?![A-Za-z]))"
);
rule!(escaped_punctuation, r"\\([&%$_#])");
rule!(whitespace_run, r"\s+");
rule!(identifier_braces, r"^\{+|\}+$");
rule!(identifier_escape, r"\\([_&#%])");
rule!(iso_date, r"^(\d{4})(?:-(\d{1,2})(?:-(\d{1,2}))?)?");
rule!(year, r"^(-?\d{1,4})$");
// ProseCraft's `/^[a-z0-9_.-]+$/i`. Without the `u` flag JavaScript folds case by upper-casing
// and never maps a non-ASCII character onto ASCII, so `ſ` (long s) does not match; `regress`
// folds it to `s`. Spelling out both cases keeps the JavaScript set.
rule!(field_name, r"^[A-Za-z0-9_.-]+$");
rule!(entry_header, r"^@([A-Za-z]+)\s*([{(])");
rule!(bad_cite_key, r"\s|[=]");
rule!(page_dashes, r"\s*[–—-]+\s*");

const LBRACE_PLACEHOLDER: &str = "\u{E001}";
const RBRACE_PLACEHOLDER: &str = "\u{E002}";

/// `BIBTEX_TYPE_MAP`: the CSL type of an entry type.
fn csl_type(entry_type: &str) -> Option<&'static str> {
    Some(match entry_type {
        "article" => "article-journal",
        "book" | "mvbook" | "collection" | "mvcollection" => "book",
        "inbook" | "incollection" | "bookinbook" => "chapter",
        "inproceedings" | "conference" | "proceedings" => "paper-conference",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "electronic" | "www" | "webpage" => "webpage",
        "unpublished" | "manuscript" => "manuscript",
        "dataset" => "dataset",
        "software" => "software",
        "misc" | "booklet" | "manual" => "document",
        _ => return None,
    })
}

/// `BUILTIN_MONTHS`: the month macros every BibTeX style defines.
fn builtin_month(name: &str) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    const NUMBERS: [&str; 12] = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
    ];
    MONTHS.iter().position(|m| *m == name).map(|i| NUMBERS[i])
}

/// `PRECOMPOSED_ACCENTS`: for each accent command, the letters it has a precomposed character
/// for, and those characters, in the same order.
const PRECOMPOSED: [(&str, &str, &str); 13] = [
    ("\"", "aeiouyAEIOUY", "äëïöüÿÄËÏÖÜŸ"),
    ("'", "acelnorsuyzACELNORSUYZ", "áćéĺńóŕśúýźÁĆÉĹŃÓŔŚÚÝŹ"),
    ("`", "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
    ("^", "aceghijosuwyACEGHIJOSUWY", "âĉêĝĥîĵôŝûŵŷÂĈÊĜĤÎĴÔŜÛŴŶ"),
    ("~", "ainouAINOU", "ãĩñõũÃĨÑÕŨ"),
    ("=", "aeiouAEIOU", "āēīōūĀĒĪŌŪ"),
    (".", "cegzCEGIZ", "ċėġżĊĖĠİŻ"),
    ("u", "aegiouAEGIOU", "ăĕğĭŏŭĂĔĞĬŎŬ"),
    ("v", "cdelnrstzCDELNRSTZ", "čďěľňřšťžČĎĚĽŇŘŠŤŽ"),
    ("H", "ouOU", "őűŐŰ"),
    ("c", "cstCST", "çşţÇŞŢ"),
    ("k", "aeiuAEIU", "ąęįųĄĘĮŲ"),
    ("r", "auAU", "åůÅŮ"),
];

fn precomposed(cmd: &str, letter: &str) -> Option<char> {
    let (_, letters, accented) = PRECOMPOSED.iter().find(|(c, _, _)| *c == cmd)?;
    let mut target = letter.chars();
    let (Some(target), None) = (target.next(), target.next()) else {
        return None;
    };
    let index = letters.chars().position(|c| c == target)?;
    accented.chars().nth(index)
}

/// `COMBINING_ACCENTS`.
fn combining(cmd: &str) -> Option<char> {
    Some(match cmd {
        "\"" => '\u{0308}',
        "'" => '\u{0301}',
        "`" => '\u{0300}',
        "^" => '\u{0302}',
        "~" => '\u{0303}',
        "=" => '\u{0304}',
        "." => '\u{0307}',
        "u" => '\u{0306}',
        "v" => '\u{030C}',
        "H" => '\u{030B}',
        "c" => '\u{0327}',
        "k" => '\u{0328}',
        "r" => '\u{030A}',
        "d" => '\u{0323}',
        "b" => '\u{0331}',
        _ => return None,
    })
}

/// `SPECIAL_MACROS`.
fn special_macro(name: &str) -> &'static str {
    match name {
        "ss" => "ß",
        "SS" => "ẞ",
        "ae" => "æ",
        "AE" => "Æ",
        "oe" => "œ",
        "OE" => "Œ",
        "aa" => "å",
        "AA" => "Å",
        "o" => "ø",
        "O" => "Ø",
        "l" => "ł",
        "L" => "Ł",
        "dh" => "ð",
        "DH" => "Ð",
        "th" => "þ",
        "TH" => "Þ",
        "i" => "ı",
        "j" => "ȷ",
        "textendash" => "–",
        "textemdash" => "—",
        "textquotedblleft" => "“",
        "textquotedblright" => "”",
        "textquoteleft" => "‘",
        "textquoteright" => "’",
        "textquotesingle" => "'",
        "textasciitilde" => "~",
        "textbackslash" => "\\",
        "ldots" | "dots" => "…",
        _ => "",
    }
}

/// `applyAccent`: the letter with the accent, precomposed where Unicode has the character.
fn apply_accent(cmd: &str, target: &str) -> String {
    let target = match target {
        "\\i" => "i",
        "\\j" => "j",
        other => other,
    };
    if let Some(c) = precomposed(cmd, target) {
        return c.to_string();
    }
    if let Some(mark) = combining(cmd)
        && target.encode_utf16().count() == 1
    {
        return format!("{target}{mark}").nfc().collect();
    }
    target.to_owned()
}

/// The first group of `groups` that took part in the match.
fn first_group<'t>(caps: &crate::js::regexp::Caps<'t>, groups: &[usize]) -> &'t str {
    groups.iter().find_map(|g| caps.get(*g)).unwrap_or_default()
}

/// The accent commands, applied until nothing changes (three passes at most).
fn decode_accents(mut text: String) -> String {
    for _ in 0..3 {
        let next = symbol_accent().replace_with(&text, true, |caps| {
            apply_accent(
                first_group(caps, &[1, 4]),
                first_group(caps, &[2, 3, 5, 6, 7, 8]),
            )
        });
        let next = letter_accent().replace_with(&next, true, |caps| {
            apply_accent(
                first_group(caps, &[1, 4, 7, 10]),
                first_group(caps, &[2, 3, 5, 6, 8, 9, 11, 12]),
            )
        });
        if next == text {
            break;
        }
        text = next;
    }
    text
}

/// `decodeLatex`: common LaTeX accents, macros and protective braces as Unicode text. With
/// `is_pages`, `--` is a hyphen (a page range) rather than an en dash.
pub fn decode_latex(input: &str, is_pages: bool) -> String {
    if input.is_empty() {
        return String::new();
    }
    let text = input
        .replace("\\{", LBRACE_PLACEHOLDER)
        .replace("\\}", RBRACE_PLACEHOLDER);
    let text = decode_accents(text);
    let text = named_macro().replace_with(&text, true, |caps| special_macro(caps.at(1)).to_owned());
    let text = escaped_punctuation().replace(&text, true, "$1");
    let text = text
        .replace("---", "—")
        .replace("--", if is_pages { "-" } else { "–" })
        .replace('~', " ");
    let text = text
        .replace(['{', '}'], "")
        .replace(LBRACE_PLACEHOLDER, "{")
        .replace(RBRACE_PLACEHOLDER, "}");
    let text = whitespace_run().replace(&text, true, " ");
    js_trim(&text).to_owned()
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `decodeRawIdentifier`: a DOI, URL or ISBN with its braces and escapes removed.
fn decode_raw_identifier(input: &str) -> String {
    let text = identifier_braces().replace(js_trim(input), true, "");
    let text = identifier_escape().replace(&text, true, "$1");
    js_trim(&text).to_owned()
}

/// The length of the character at byte `i` (1 past the end).
fn char_len(text: &str, i: usize) -> usize {
    text[i..].chars().next().map_or(1, char::len_utf8)
}

/// The separators `splitAtDepthZero` is given: `/\s+and\s+/iy` and `/,/y`, matched at `i`.
#[derive(Clone, Copy)]
enum Separator {
    And,
    Comma,
}

impl Separator {
    /// The length of the separator at byte `i`, if one starts there.
    fn at(self, text: &str, i: usize) -> Option<usize> {
        let rest = &text[i..];
        match self {
            Self::Comma => rest.starts_with(',').then_some(1),
            Self::And => {
                let spaces = |s: &str| {
                    s.char_indices()
                        .find(|(_, c)| !is_js_whitespace(*c))
                        .map_or(s.len(), |(at, _)| at)
                };
                let before = spaces(rest);
                let word = rest.get(before..before + 3)?;
                if before == 0 || !word.eq_ignore_ascii_case("and") {
                    return None;
                }
                let after = spaces(&rest[before + 3..]);
                (after > 0).then_some(before + 3 + after)
            }
        }
    }
}

/// `splitAtDepthZero`: `input` cut at each separator outside braces.
fn split_at_depth_zero(input: &str, separator: Separator) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                if i < bytes.len() {
                    i += char_len(input, i);
                }
                continue;
            }
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                if let Some(len) = separator.at(input, i) {
                    parts.push(&input[last..i]);
                    i += len;
                    last = i;
                    continue;
                }
            }
            _ => {}
        }
        i += char_len(input, i);
    }
    parts.push(&input[last..]);
    parts
}

/// `isSingleOuterBracedGroup`: the whole text is one `{…}` group.
fn is_single_outer_braced_group(text: &str) -> bool {
    if !text.starts_with('{') || !text.ends_with('}') {
        return false;
    }
    let bytes = text.as_bytes();
    let mut depth: i64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 && i < bytes.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth == 0
}

fn text_or_none(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// A name object of family, given and suffix (each when not empty), or `None` without a family
/// or given name.
fn name(family: String, given: String, suffix: String) -> Option<JsValue> {
    if family.is_empty() && given.is_empty() {
        return None;
    }
    let mut entry = JsObject::new();
    for (key, value) in [("family", family), ("given", given), ("suffix", suffix)] {
        if !value.is_empty() {
            entry.insert(key, JsValue::str(&value));
        }
    }
    Some(JsValue::Object(entry))
}

/// One `and`-separated name of an author or editor field.
fn parse_bibtex_name(chunk: &str) -> Option<JsValue> {
    if is_single_outer_braced_group(chunk) {
        let literal = decode_latex(chunk, false);
        if literal.is_empty() {
            return None;
        }
        let mut entry = JsObject::new();
        entry.insert("literal", JsValue::str(&literal));
        return Some(JsValue::Object(entry));
    }
    let comma_parts = split_at_depth_zero(chunk, Separator::Comma);
    match comma_parts.as_slice() {
        [family, suffix, given @ ..] if !given.is_empty() => name(
            decode_latex(family, false),
            decode_latex(&given.join(", "), false),
            decode_latex(suffix, false),
        ),
        [family, given] => name(
            decode_latex(family, false),
            decode_latex(given, false),
            String::new(),
        ),
        _ => parse_person_name(&decode_latex(chunk, false)),
    }
}

/// `parseBibtexAuthors`: an author or editor field as CSL names, `None` when it has none.
fn parse_bibtex_authors(raw: &str) -> Option<JsValue> {
    let names: Vec<JsValue> = split_at_depth_zero(js_trim(raw), Separator::And)
        .into_iter()
        .map(js_trim)
        .filter(|chunk| !chunk.is_empty())
        .filter_map(parse_bibtex_name)
        .collect();
    (!names.is_empty()).then_some(JsValue::Array(names))
}

fn date_parts(parts: Vec<JsValue>) -> JsValue {
    let mut date = JsObject::new();
    date.insert("date-parts", JsValue::Array(vec![JsValue::Array(parts)]));
    JsValue::Object(date)
}

fn literal_date(text: &str) -> JsValue {
    let mut date = JsObject::new();
    date.insert("literal", JsValue::str(text));
    JsValue::Object(date)
}

/// `parseBibtexIssued`: the date from `date`, else from `year` and `month`.
fn parse_bibtex_issued(fields: &HashMap<String, String>) -> Option<JsValue> {
    let decoded = |key: &str| {
        fields
            .get(key)
            .filter(|v| !v.is_empty())
            .map(|v| decode_latex(v, false))
            .unwrap_or_default()
    };
    let raw_date = decoded("date");
    if !raw_date.is_empty()
        && let Some(caps) = iso_date().exec(js_trim(&raw_date))
    {
        let parts = [1, 2, 3]
            .iter()
            .filter_map(|g| caps.get(*g).filter(|p| !p.is_empty()))
            .map(|p| JsValue::Number(string_to_number(p)))
            .collect();
        return Some(date_parts(parts));
    }
    let raw_year = decoded("year");
    let raw_month = decoded("month");
    if raw_year.is_empty() {
        return (!raw_date.is_empty()).then(|| literal_date(&raw_date));
    }
    let Some(caps) = year().exec(js_trim(&raw_year)) else {
        let literal = if raw_month.is_empty() {
            raw_year
        } else {
            format!("{raw_month} {raw_year}")
        };
        return Some(literal_date(&literal));
    };
    let mut parts = vec![JsValue::Number(string_to_number(caps.at(1)))];
    if !raw_month.is_empty()
        && let Some(month) = parse_month_value(&raw_month)
    {
        parts.push(month);
    }
    Some(date_parts(parts))
}

/// Walks `text` from byte `from` as the entry scanners do: a backslash escapes the next
/// character, braces nest, and `"` opens a quoted value at depth zero. `stop` is called for
/// every other character with its position and whether it is outside braces and quotes.
fn scan_depth_zero(text: &str, mut stop: impl FnMut(usize, u8, bool) -> bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'\\' && i + 1 < bytes.len() {
            i += 1 + char_len(text, i + 1);
            continue;
        }
        match ch {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'"' if depth == 0 => in_quotes = !in_quotes,
            _ => {
                if stop(i, ch, depth == 0 && !in_quotes) {
                    return Some(i);
                }
            }
        }
        i += char_len(text, i);
    }
    None
}

/// `splitEntryFields` (and the `#` split of `evaluateValueExpression`): `body` cut at each
/// `separator` outside braces and quotes.
fn split_depth_zero(body: &str, separator: u8) -> Vec<&str> {
    let mut cuts = Vec::new();
    scan_depth_zero(body, |i, ch, top| {
        if top && ch == separator {
            cuts.push(i);
        }
        false
    });
    let mut parts = Vec::new();
    let mut start = 0;
    for cut in cuts {
        parts.push(&body[start..cut]);
        start = cut + 1;
    }
    parts.push(&body[start..]);
    parts
}

/// `findDepthZeroChar`: the first `target` outside braces and quotes.
fn find_depth_zero_char(text: &str, target: u8) -> Option<usize> {
    scan_depth_zero(text, |_, ch, top| top && ch == target)
}

/// `evaluateValueExpression`: a field value, its `#`-joined parts concatenated and macros
/// (`@string` definitions and month names) expanded.
fn evaluate_value_expression(expr: &str, macros: &HashMap<String, String>) -> String {
    split_depth_zero(expr, b'#')
        .into_iter()
        .map(|raw| {
            let token = js_trim(raw);
            if token.is_empty() {
                return String::new();
            }
            if (token.starts_with('{') && token.ends_with('}'))
                || (token.starts_with('"') && token.ends_with('"'))
            {
                return token.get(1..token.len() - 1).unwrap_or_default().to_owned();
            }
            let lower = token.to_lowercase();
            if let Some(value) = macros.get(&lower) {
                return value.clone();
            }
            builtin_month(&lower)
                .or_else(|| Inherited::of(&lower).map(Inherited::text))
                .unwrap_or(token)
                .to_owned()
        })
        .collect()
}

/// `scanEntryDelimiter`: the position of the `}` or `)` that closes the entry opened at
/// `open_index`.
fn scan_entry_delimiter(text: &str, open_index: usize, open_char: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let base_depth: i64 = if open_char == b'{' { 1 } else { 0 };
    let mut depth = base_depth;
    let mut paren_depth: i64 = if open_char == b'(' { 1 } else { 0 };
    let mut in_quotes = false;
    let mut i = open_index + 1;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'\\' && i + 1 < bytes.len() {
            i += 1 + char_len(text, i + 1);
            continue;
        }
        match ch {
            b'"' if depth == base_depth => in_quotes = !in_quotes,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if open_char == b'{' && !in_quotes && depth == 0 {
                    return Some(i);
                }
                depth = depth.max(0);
            }
            b'(' if open_char == b'(' && !in_quotes && depth == 0 => paren_depth += 1,
            b')' if open_char == b'(' && !in_quotes && depth == 0 => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += char_len(text, i);
    }
    None
}

/// The first `count` UTF-16 units of `text`, as `slice(0, count)` cuts it.
fn first_units(text: &str, count: usize) -> String {
    String::from_utf16_lossy(&text.encode_utf16().take(count).collect::<Vec<_>>())
}

/// A byte position as the UTF-16 offset JavaScript reports.
fn js_offset(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}

/// `parseFieldAssignments`: the `name = value` fields of an entry, names lower-cased; later
/// ones replace earlier ones of the same name.
fn parse_field_assignments(
    body: &str,
    macros: &HashMap<String, String>,
    context_label: &str,
    warnings: &mut Vec<String>,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for segment in split_depth_zero(body, b',') {
        let trimmed = js_trim(segment);
        if trimmed.is_empty() {
            continue;
        }
        let Some(eq) = find_depth_zero_char(trimmed, b'=').filter(|eq| *eq > 0) else {
            warnings.push(format!(
                "Malformed field in {context_label}: \"{}\".",
                first_units(trimmed, 60)
            ));
            continue;
        };
        let name = js_trim(&trimmed[..eq]).to_lowercase();
        let value_expr = js_trim(&trimmed[eq + 1..]);
        if !field_name().test(&name) {
            warnings.push(format!("Invalid field name \"{name}\" in {context_label}."));
            continue;
        }
        fields.insert(name, evaluate_value_expression(value_expr, macros));
    }
    fields
}

/// The fields of one entry, looked up as the TypeScript does (`fields.x ?? fields.y`).
struct Fields(HashMap<String, String>);

impl Fields {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The first of `keys` present (even if empty), decoded; `None` when that is empty.
    fn decoded(&self, keys: &[&str]) -> Option<String> {
        let raw = keys.iter().find_map(|k| self.get(k))?;
        text_or_none(decode_latex(raw, false))
    }

    fn identifier(&self, key: &str) -> Option<String> {
        self.get(key)
            .filter(|raw| !raw.is_empty())
            .and_then(|raw| text_or_none(decode_raw_identifier(raw)))
    }
}

/// The CSL item of an entry with its cite key, type and fields.
fn entry_item(cite_key: &str, entry_type: &str, fields: &Fields) -> JsValue {
    let mut item = JsObject::new();
    item.insert("id", JsValue::str(cite_key));
    // `BIBTEX_TYPE_MAP.constructor` is a function: JSON leaves the key out, and
    // normalization finds no type.
    if Inherited::of(entry_type).is_none() {
        item.insert(
            "type",
            JsValue::str(csl_type(entry_type).unwrap_or("document")),
        );
    }
    item.insert("citeKey", JsValue::str(cite_key));
    item.insert("provider", JsValue::str("import"));
    let is_book = entry_type == "book";
    let text = |v: Option<String>| v.map(|t| JsValue::str(&t));
    let title_keys: &[&str] = if is_book {
        &["title", "booktitle"]
    } else {
        &["title"]
    };
    let container_keys: &[&str] = if is_book {
        &["journal", "journaltitle"]
    } else {
        &["journal", "journaltitle", "booktitle"]
    };
    let authors = |key: &str| {
        fields
            .get(key)
            .filter(|v| !v.is_empty())
            .and_then(parse_bibtex_authors)
    };
    let page = fields
        .get("pages")
        .filter(|v| !v.is_empty())
        .map(|raw| page_dashes().replace(&decode_latex(raw, true), true, "-"))
        .and_then(text_or_none);
    let entries = [
        ("title", text(fields.decoded(title_keys))),
        ("author", authors("author")),
        ("editor", authors("editor")),
        ("issued", parse_bibtex_issued(&fields.0)),
        ("container-title", text(fields.decoded(container_keys))),
        (
            "publisher",
            text(fields.decoded(&["publisher", "school", "institution", "organization"])),
        ),
        (
            "publisher-place",
            text(fields.decoded(&["address", "location"])),
        ),
        ("volume", text(fields.decoded(&["volume"]))),
        ("issue", text(fields.decoded(&["issue", "number"]))),
        ("page", text(page)),
        ("DOI", text(fields.identifier("doi"))),
        ("URL", text(fields.identifier("url"))),
        ("ISBN", text(fields.identifier("isbn"))),
        ("edition", text(fields.decoded(&["edition"]))),
        ("language", text(fields.decoded(&["language", "langid"]))),
        ("abstract-free", text(fields.decoded(&["abstract"]))),
    ];
    for (key, value) in entries {
        if let Some(value) = value {
            item.insert(key, value);
        }
    }
    JsValue::Object(item)
}

/// The state of a parse: macros defined so far, items and warnings.
struct Parser {
    macros: HashMap<String, String>,
    items: Vec<JsValue>,
    warnings: Vec<String>,
}

impl Parser {
    /// An `@string{key = value}` definition.
    fn define_macro(&mut self, body: &str) {
        let Some(eq) = find_depth_zero_char(body, b'=').filter(|eq| *eq > 0) else {
            self.warnings
                .push("Malformed @string macro definition skipped.".into());
            return;
        };
        let key = js_trim(&body[..eq]).to_lowercase();
        let expr = js_trim(&body[eq + 1..]);
        if !field_name().test(&key) {
            self.warnings
                .push(format!("Invalid @string macro key \"{key}\" skipped."));
            return;
        }
        let value = evaluate_value_expression(expr, &self.macros);
        self.macros.insert(key, value);
    }

    /// An entry `@type{citekey, fields…}`.
    fn entry(&mut self, raw_type: &str, entry_type: &str, body: &str) {
        let Some(comma) = find_depth_zero_char(body, b',') else {
            self.warnings.push(format!(
                "Malformed BibTeX \"@{raw_type}\" entry (missing citekey comma) skipped."
            ));
            return;
        };
        let cite_key = js_trim(&body[..comma]);
        if cite_key.is_empty() || bad_cite_key().test(cite_key) {
            self.warnings.push(format!(
                "Malformed BibTeX \"@{raw_type}\" entry (invalid citekey) skipped."
            ));
            return;
        }
        let fields = parse_field_assignments(
            &body[comma + 1..],
            &self.macros,
            &format!("@{raw_type}{{{cite_key}}}"),
            &mut self.warnings,
        );
        self.items
            .push(entry_item(cite_key, entry_type, &Fields(fields)));
    }
}

/// `parseBibtex`: the entries of BibTeX or BibLaTeX text as CSL-JSON items, and a warning for
/// each construct that could not be read.
pub fn parse_bibtex(text: &str) -> BibtexParse {
    let mut parser = Parser {
        macros: HashMap::new(),
        items: Vec::new(),
        warnings: Vec::new(),
    };
    let mut index = 0;
    while index < text.len() {
        let Some(at) = text[index..].find('@').map(|i| i + index) else {
            break;
        };
        let Some(header) = entry_header().exec(&text[at..]) else {
            parser.warnings.push(format!(
                "Malformed BibTeX \"@\" construct near offset {}.",
                js_offset(text, at)
            ));
            index = at + 1;
            continue;
        };
        let raw_type = header.at(1);
        let entry_type = raw_type.to_lowercase();
        let open_char = header.at(2).as_bytes().first().copied().unwrap_or(b'{');
        let open_index = at + header.end() - 1;
        let Some(close) = scan_entry_delimiter(text, open_index, open_char) else {
            parser.warnings.push(format!(
                "Unclosed BibTeX \"@{raw_type}\" entry near offset {}.",
                js_offset(text, at)
            ));
            break;
        };
        let body = &text[open_index + 1..close];
        index = close + 1;
        match entry_type.as_str() {
            "comment" | "preamble" => {}
            "string" => parser.define_macro(body),
            _ => parser.entry(raw_type, &entry_type, body),
        }
    }
    BibtexParse {
        items: parser.items,
        warnings: parser.warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_compiles() {
        for rule in [
            symbol_accent(),
            letter_accent(),
            named_macro(),
            escaped_punctuation(),
            whitespace_run(),
            identifier_braces(),
            identifier_escape(),
            iso_date(),
            year(),
            field_name(),
            entry_header(),
            bad_cite_key(),
            page_dashes(),
        ] {
            assert!(rule.compiled());
        }
    }

    #[test]
    fn precomposed_tables_line_up() {
        for (cmd, letters, accented) in PRECOMPOSED {
            assert_eq!(letters.chars().count(), accented.chars().count(), "{cmd}");
        }
    }
}
