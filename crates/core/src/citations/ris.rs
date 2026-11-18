//! RIS import (ProseCraft `src/lib/citations/ris.ts`, upstream `7dae544`): `TY` … `ER` records
//! read into CSL-JSON items, and a warning for each line or record that could not be read.

use crate::csl::{parse_month_number, parse_person_name};
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsValue, string_to_number};
use std::sync::OnceLock;

/// Items read from an RIS file, and why parts of it were not.
#[derive(Clone, Debug, PartialEq)]
pub struct RisParse {
    pub items: Vec<JsValue>,
    pub warnings: Vec<String>,
}

fn ris_tag() -> &'static Re {
    static RULE: OnceLock<Re> = OnceLock::new();
    RULE.get_or_init(|| Re::new(r"^([A-Z0-9]{2})\s{1,2}-(?:\s(.*))?$", ""))
}

/// `RIS_TYPE_MAP`: the CSL type of an RIS reference type.
fn csl_type(ris_type: &str) -> Option<&'static str> {
    Some(match ris_type {
        "BOOK" | "WHOLE" | "EDBOOK" => "book",
        "JOUR" | "JFULL" | "EJOUR" => "article-journal",
        "MGZN" => "article-magazine",
        "NEWS" => "article-newspaper",
        "CHAP" | "SECT" => "chapter",
        "CONF" | "CPAPER" => "paper-conference",
        "THES" => "thesis",
        "RPRT" => "report",
        "ELEC" | "WEB" => "webpage",
        "ENCYC" => "entry-encyclopedia",
        "DICT" => "entry-dictionary",
        "MANSCPT" | "UNPB" => "manuscript",
        "DATA" => "dataset",
        "COMP" => "software",
        "GEN" | "MISC" | "STD" => "document",
        "PAT" => "patent",
        _ => return None,
    })
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

fn is_digits(text: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
    lengths.contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

/// `parseRisDate`: year, month and day from `YYYY/MM/DD/other` (or with dashes), else the text
/// as a literal date. Returns the date and how many parts it has (0 for a literal).
fn parse_ris_date(raw: &str) -> Option<(JsValue, usize)> {
    let trimmed = js_trim(raw);
    if trimmed.is_empty() {
        return None;
    }
    let segments: Vec<&str> = trimmed.split(['/', '-']).collect();
    let mut date = JsObject::new();
    if !is_digits(segments[0], 4..=4) {
        date.insert("literal", JsValue::str(trimmed));
        return Some((JsValue::Object(date), 0));
    }
    let mut parts = vec![JsValue::Number(string_to_number(segments[0]))];
    let raw_month = js_trim(segments.get(1).copied().unwrap_or_default());
    let month = (!raw_month.is_empty())
        .then(|| parse_month_number(raw_month))
        .flatten();
    if let Some(month) = month {
        parts.push(JsValue::Number(month));
        let raw_day = js_trim(segments.get(2).copied().unwrap_or_default());
        if is_digits(raw_day, 1..=2) {
            let day = string_to_number(raw_day);
            if (1.0..=31.0).contains(&day) {
                parts.push(JsValue::Number(day));
            }
        }
    }
    let count = parts.len();
    date.insert("date-parts", JsValue::Array(vec![JsValue::Array(parts)]));
    Some((JsValue::Object(date), count))
}

/// `chooseBestRisDate`: the candidate with the most date parts (the first of equals).
fn choose_best_ris_date(candidates: &[String]) -> Option<JsValue> {
    let mut best: Option<(JsValue, usize)> = None;
    for raw in candidates {
        let Some((parsed, score)) = parse_ris_date(raw) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            best = Some((parsed, score));
        }
    }
    best.map(|(date, _)| date)
}

/// A record between `TY` and `ER`.
struct Draft {
    start_line: usize,
    ris_type: String,
    tags: Vec<(String, String)>,
}

/// The fields of a record: the first value of each tag (names and dates: all of them).
#[derive(Default)]
struct Record {
    authors: Vec<JsValue>,
    editors: Vec<JsValue>,
    dates: Vec<String>,
    title: Option<String>,
    t1: Option<String>,
    t2: Option<String>,
    jo: Option<String>,
    jf: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    sp: Option<String>,
    ep: Option<String>,
    publisher: Option<String>,
    publisher_place: Option<String>,
    doi: Option<String>,
    url: Option<String>,
    isbn: Option<String>,
    id: Option<String>,
    language: Option<String>,
    edition: Option<String>,
    abstract_free: Option<String>,
}

impl Record {
    fn slot(&mut self, tag: &str) -> Option<&mut Option<String>> {
        Some(match tag {
            "TI" => &mut self.title,
            "T1" => &mut self.t1,
            "T2" | "BT" => &mut self.t2,
            "JO" | "J2" => &mut self.jo,
            "JF" => &mut self.jf,
            "VL" => &mut self.volume,
            "IS" => &mut self.issue,
            "SP" => &mut self.sp,
            "EP" => &mut self.ep,
            "PB" => &mut self.publisher,
            "CY" => &mut self.publisher_place,
            "DO" => &mut self.doi,
            "UR" => &mut self.url,
            "SN" => &mut self.isbn,
            "ID" => &mut self.id,
            "LA" => &mut self.language,
            "ET" => &mut self.edition,
            "AB" | "N2" => &mut self.abstract_free,
            _ => return None,
        })
    }

    fn add(&mut self, tag: &str, value: &str) {
        match tag {
            "AU" | "A1" => self.authors.extend(parse_person_name(value)),
            "A2" | "ED" => self.editors.extend(parse_person_name(value)),
            "PY" | "Y1" | "DA" => self.dates.push(value.to_owned()),
            _ => {
                if let Some(slot) = self.slot(tag)
                    && slot.is_none()
                {
                    *slot = Some(value.to_owned());
                }
            }
        }
    }
}

/// `recordToCslItem`: the CSL item of a record, or why it has none.
fn record_to_csl_item(draft: &Draft) -> Result<JsValue, String> {
    let normalized_type = js_trim(&draft.ris_type).to_uppercase();
    if normalized_type.is_empty() {
        return Err(format!(
            "RIS record at line {} has an empty TY tag.",
            draft.start_line
        ));
    }
    let csl_type = csl_type(&normalized_type).unwrap_or("document");
    let mut record = Record::default();
    for (tag, value) in &draft.tags {
        let trimmed = js_trim(value);
        if !trimmed.is_empty() {
            record.add(tag, trimmed);
        }
    }
    let r = record;
    let final_title = r.title.or(r.t1);
    let container_title = if csl_type == "chapter" || csl_type == "paper-conference" {
        r.t2.clone()
            .or_else(|| r.jf.clone())
            .or_else(|| r.jo.clone())
    } else {
        r.jf.clone()
            .or_else(|| r.jo.clone())
            .or_else(|| r.t2.clone())
    };
    let issued = choose_best_ris_date(&r.dates);
    let page = match (&r.sp, &r.ep) {
        (Some(sp), Some(ep)) => Some(format!("{sp}-{ep}")),
        (sp, ep) => sp.clone().or_else(|| ep.clone()),
    };
    let has_content = final_title.is_some()
        || !r.authors.is_empty()
        || !r.editors.is_empty()
        || container_title.is_some()
        || r.publisher.is_some()
        || r.doi.is_some()
        || r.url.is_some()
        || r.isbn.is_some()
        || issued.is_some()
        || r.abstract_free.is_some();
    if !has_content {
        return Err(format!(
            "RIS record at line {} has no bibliographic fields.",
            draft.start_line
        ));
    }
    let mut item = JsObject::new();
    item.insert("id", JsValue::str(r.id.as_deref().unwrap_or_default()));
    item.insert("type", JsValue::str(csl_type));
    item.insert("provider", JsValue::str("import"));
    let text = |v: Option<String>| v.map(|t| JsValue::str(&t));
    let list = |v: Vec<JsValue>| (!v.is_empty()).then_some(JsValue::Array(v));
    let entries = [
        ("citeKey", text(r.id)),
        ("title", text(final_title)),
        ("author", list(r.authors)),
        ("editor", list(r.editors)),
        ("issued", issued),
        ("container-title", text(container_title)),
        ("publisher", text(r.publisher)),
        ("publisher-place", text(r.publisher_place)),
        ("volume", text(r.volume)),
        ("issue", text(r.issue)),
        ("page", text(page)),
        ("DOI", text(r.doi)),
        ("URL", text(r.url)),
        ("ISBN", text(r.isbn)),
        ("edition", text(r.edition)),
        ("language", text(r.language)),
        ("abstract-free", text(r.abstract_free)),
    ];
    for (key, value) in entries {
        if let Some(value) = value {
            item.insert(key, value);
        }
    }
    Ok(JsValue::Object(item))
}

/// The state of a parse: the open record, items and warnings.
#[derive(Default)]
struct Parser {
    current: Option<Draft>,
    outside_garbage_warned: bool,
    items: Vec<JsValue>,
    warnings: Vec<String>,
}

impl Parser {
    fn tag(&mut self, line_num: usize, tag: &str, value: &str) {
        self.outside_garbage_warned = false;
        match tag {
            "TY" => {
                if let Some(current) = &self.current {
                    self.warnings.push(format!(
                        "RIS record starting at line {} was missing \"ER  -\" before line {line_num}; skipped.",
                        current.start_line
                    ));
                }
                self.current = Some(Draft {
                    start_line: line_num,
                    ris_type: js_trim(value).to_owned(),
                    tags: Vec::new(),
                });
            }
            "ER" => {
                let Some(current) = self.current.take() else {
                    self.warnings.push(format!(
                        "Unexpected RIS \"ER\" tag at line {line_num} outside a record."
                    ));
                    return;
                };
                match record_to_csl_item(&current) {
                    Ok(item) => self.items.push(item),
                    Err(error) => self.warnings.push(error),
                }
            }
            _ => match &mut self.current {
                Some(current) => current
                    .tags
                    .push((tag.to_owned(), js_trim(value).to_owned())),
                None => self.warnings.push(format!(
                    "RIS tag \"{tag}\" at line {line_num} appeared outside a TY/ER record; ignored."
                )),
            },
        }
    }

    fn other_line(&mut self, line_num: usize, raw_line: &str) {
        let trimmed_line = js_trim(raw_line);
        if trimmed_line.is_empty() {
            return;
        }
        let Some(current) = &mut self.current else {
            if !self.outside_garbage_warned {
                self.warnings
                    .push(format!("Ignored non-RIS text starting at line {line_num}."));
                self.outside_garbage_warned = true;
            }
            return;
        };
        match current.tags.last_mut() {
            Some((_, prev)) => {
                *prev = if prev.is_empty() {
                    trimmed_line.to_owned()
                } else {
                    format!("{prev} {trimmed_line}")
                };
            }
            None => self.warnings.push(format!(
                "Unexpected continuation line at line {line_num} before any field tag."
            )),
        }
    }
}

/// `parseRis`: the records of RIS text as CSL-JSON items, and a warning for each line or record
/// that could not be read.
pub fn parse_ris(text: &str) -> RisParse {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut parser = Parser::default();
    for (i, raw_line) in normalized.split('\n').enumerate() {
        let line_num = i + 1;
        match ris_tag().exec(raw_line) {
            Some(caps) => {
                let value = caps.at(2).trim_end_matches(is_js_whitespace);
                parser.tag(line_num, caps.at(1), value);
            }
            None => parser.other_line(line_num, raw_line),
        }
    }
    if let Some(current) = &parser.current {
        parser.warnings.push(format!(
            "RIS record starting at line {} is missing closing \"ER  -\"; skipped.",
            current.start_line
        ));
    }
    RisParse {
        items: parser.items,
        warnings: parser.warnings,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_tag_rule_compiles() {
        assert!(super::ris_tag().compiled());
    }
}
