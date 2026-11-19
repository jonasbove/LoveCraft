//! Story metadata on documents (ProseCraft `src/lib/story-meta.ts` and `story-yaml.ts`,
//! upstream `bcb884d`): a Wiki page's kind, aliases, tags, sheet values and relations, and a
//! scene's confirmed characters, place and story date.
//!
//! The metadata stays a [`JsValue`] in the stored project, as everything else does; this module
//! normalizes it ([`normalize_meta`]) and reads typed values out of it ([`Meta`]).

use crate::js::{JsObject, JsString, JsValue};
use crate::sync::open_layout::{format_yaml_value, parse_yaml_value};

pub const PAGE_KINDS: [&str; 5] = ["character", "place", "organisation", "item", "other"];
pub const RELATION_TYPES: [&str; 7] = [
    "family", "friend", "rival", "love", "mentor", "employer", "other",
];
pub const SHEET_FIELD_TYPES: [&str; 5] = ["text", "longtext", "number", "choice", "link"];

const MAX_RELATIONS: usize = 200;
const MAX_ALIASES: usize = 20;
const MAX_TAGS: usize = 50;
const MAX_CHARACTERS: usize = 200;
const MAX_SHEET_ENTRIES: usize = 200;
const MAX_ID: usize = 200;
const MAX_LABEL: usize = 120;
const MAX_NOTE: usize = 2000;
const MAX_ALIAS: usize = 120;
const MAX_PLACE: usize = 200;
const MAX_STORY_DATE: usize = 40;
const MAX_TAG: usize = 60;
const MAX_SHEET_VALUE: usize = 8000;

const KNOWN_KEYS: [&str; 9] = [
    "kind",
    "aliases",
    "characters",
    "place",
    "storyDate",
    "tags",
    "sheet",
    "sheetTemplate",
    "relations",
];

fn is_space(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(crate::html::is_js_whitespace)
}

/// `String.prototype.trim` on UTF-16 units.
pub(crate) fn trim_units(units: &[u16]) -> &[u16] {
    let start = units
        .iter()
        .position(|u| !is_space(*u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|u| !is_space(*u))
        .map_or(start, |e| e + 1);
    &units[start..end]
}

fn js_text(value: Option<&JsValue>) -> Option<&JsString> {
    value.and_then(JsValue::as_js_string)
}

/// `readId`: a trimmed id of at most 200 units.
fn read_id(value: Option<&JsValue>) -> Option<JsString> {
    let id = trim_units(js_text(value)?.units());
    (!id.is_empty() && id.len() <= MAX_ID).then(|| JsString::from_units(id.to_vec()))
}

/// `readText`: trimmed, clipped to `max` units; empty is dropped.
fn read_text(value: Option<&JsValue>, max: usize) -> Option<JsString> {
    let text = trim_units(js_text(value)?.units());
    let text = &text[..text.len().min(max)];
    (!text.is_empty()).then(|| JsString::from_units(text.to_vec()))
}

fn read_string_list(value: Option<&JsValue>, limit: usize, max: usize) -> Vec<JsValue> {
    let mut out = Vec::new();
    for item in value.and_then(JsValue::as_array).unwrap_or_default() {
        if let Some(text) = read_text(Some(item), max) {
            out.push(JsValue::String(text));
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// `readTension`: rounded (half up, as `Math.round`) and clamped to 1..=5.
fn read_tension(value: Option<&JsValue>) -> Option<f64> {
    let n = value?.as_number().filter(|n| n.is_finite())?;
    Some((n + 0.5).floor().clamp(1.0, 5.0))
}

fn one_of(value: Option<&JsValue>, list: &[&str]) -> Option<JsString> {
    let text = js_text(value)?;
    list.iter().any(|k| text.eq_str(k)).then(|| text.clone())
}

fn normalize_relation(value: &JsValue) -> Option<JsValue> {
    value.as_object()?;
    let id = read_id(value.get("id"))?;
    let to = read_id(value.get("to"))?;
    let kind = one_of(value.get("type"), &RELATION_TYPES)?;
    let mut relation = JsObject::new();
    relation.insert("id", JsValue::String(id));
    relation.insert("to", JsValue::String(to));
    relation.insert("type", JsValue::String(kind));
    if let Some(label) = read_text(value.get("label"), MAX_LABEL) {
        relation.insert("label", JsValue::String(label));
    }
    if let Some(tension) = read_tension(value.get("tension")) {
        relation.insert("tension", JsValue::Number(tension));
    }
    if let Some(from) = read_id(value.get("from")) {
        relation.insert("from", JsValue::String(from));
    }
    if let Some(until) = read_id(value.get("until")) {
        relation.insert("until", JsValue::String(until));
    }
    if let Some(note) = read_text(value.get("note"), MAX_NOTE) {
        relation.insert("note", JsValue::String(note));
    }
    Some(JsValue::Object(relation))
}

fn read_sheet(value: Option<&JsValue>) -> Option<JsObject> {
    let source = value?.as_object()?;
    let mut sheet = JsObject::new();
    for (key, entry) in source.iter() {
        if key.is_empty() {
            continue;
        }
        let Some(text) = entry.as_js_string() else {
            continue;
        };
        let units = text.units();
        sheet.insert(
            key.clone(),
            JsValue::String(JsString::from_units(
                units[..units.len().min(MAX_SHEET_VALUE)].to_vec(),
            )),
        );
        if sheet.len() >= MAX_SHEET_ENTRIES {
            break;
        }
    }
    (!sheet.is_empty()).then_some(sheet)
}

/// `normalizeMeta`: invalid values dropped, lists capped, unknown keys kept. `None` when nothing
/// usable is left (or the value isn't an object).
pub fn normalize_meta(value: &JsValue) -> Option<JsValue> {
    let source = value.as_object()?;
    let mut meta = JsObject::new();
    if let Some(kind) = one_of(value.get("kind"), &PAGE_KINDS) {
        meta.insert("kind", JsValue::String(kind));
    }
    for (key, limit, max) in [
        ("aliases", MAX_ALIASES, MAX_ALIAS),
        ("characters", MAX_CHARACTERS, MAX_ID),
    ] {
        let list = read_string_list(value.get(key), limit, max);
        if !list.is_empty() {
            meta.insert(key, JsValue::Array(list));
        }
    }
    if let Some(place) = read_text(value.get("place"), MAX_PLACE) {
        meta.insert("place", JsValue::String(place));
    }
    if let Some(date) = read_text(value.get("storyDate"), MAX_STORY_DATE) {
        meta.insert("storyDate", JsValue::String(date));
    }
    let tags = read_string_list(value.get("tags"), MAX_TAGS, MAX_TAG);
    if !tags.is_empty() {
        meta.insert("tags", JsValue::Array(tags));
    }
    if let Some(sheet) = read_sheet(value.get("sheet")) {
        meta.insert("sheet", JsValue::Object(sheet));
    }
    if let Some(template) = read_id(value.get("sheetTemplate")) {
        meta.insert("sheetTemplate", JsValue::String(template));
    }
    let mut relations = Vec::new();
    for item in value
        .get("relations")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        if let Some(relation) = normalize_relation(item) {
            relations.push(relation);
        }
        if relations.len() >= MAX_RELATIONS {
            break;
        }
    }
    if !relations.is_empty() {
        meta.insert("relations", JsValue::Array(relations));
    }
    for (key, entry) in source.iter() {
        if KNOWN_KEYS.iter().any(|k| key.eq_str(k)) {
            continue;
        }
        meta.insert(key.clone(), entry.clone());
    }
    (!meta.is_empty()).then_some(JsValue::Object(meta))
}

/// A stored relation (`Relation`).
#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub id: String,
    pub to: String,
    pub kind: String,
    pub label: Option<String>,
    pub tension: Option<f64>,
    pub from: Option<String>,
    pub until: Option<String>,
    pub note: Option<String>,
}

fn string(value: Option<&JsValue>) -> Option<String> {
    value
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
}

impl Relation {
    pub fn from_js(value: &JsValue) -> Option<Self> {
        Some(Self {
            id: string(value.get("id"))?,
            to: string(value.get("to"))?,
            kind: string(value.get("type")).unwrap_or_else(|| "other".into()),
            label: string(value.get("label")),
            tension: value.get("tension").and_then(JsValue::as_number),
            from: string(value.get("from")),
            until: string(value.get("until")),
            note: string(value.get("note")),
        })
    }

    pub fn to_js(&self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("id", JsValue::str(&self.id));
        o.insert("to", JsValue::str(&self.to));
        o.insert("type", JsValue::str(&self.kind));
        let mut opt = |key: &str, v: &Option<String>| {
            if let Some(v) = v {
                o.insert(key, JsValue::str(v));
            }
        };
        opt("label", &self.label);
        if let Some(t) = self.tension {
            o.insert("tension", JsValue::Number(t));
        }
        let mut opt = |key: &str, v: &Option<String>| {
            if let Some(v) = v {
                o.insert(key, JsValue::str(v));
            }
        };
        opt("from", &self.from);
        opt("until", &self.until);
        opt("note", &self.note);
        JsValue::Object(o)
    }
}

/// Typed reads of a document's `meta` (as stored; normalized when the project was read).
#[derive(Clone, Copy, Debug)]
pub struct Meta<'a>(pub Option<&'a JsValue>);

impl<'a> Meta<'a> {
    pub fn of(document: &'a JsValue) -> Self {
        Self(document.get("meta").filter(|m| m.as_object().is_some()))
    }

    fn get(&self, key: &str) -> Option<&'a JsValue> {
        self.0.and_then(|m| m.get(key))
    }

    pub fn kind(&self) -> Option<String> {
        string(self.get("kind")).filter(|k| PAGE_KINDS.contains(&k.as_str()))
    }

    fn strings(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|v| string(Some(v)))
            .collect()
    }

    pub fn aliases(&self) -> Vec<String> {
        self.strings("aliases")
    }

    pub fn characters(&self) -> Vec<String> {
        self.strings("characters")
    }

    pub fn tags(&self) -> Vec<String> {
        self.strings("tags")
    }

    pub fn place(&self) -> Option<String> {
        string(self.get("place"))
    }

    pub fn story_date(&self) -> Option<String> {
        string(self.get("storyDate"))
    }

    pub fn sheet_template(&self) -> Option<String> {
        string(self.get("sheetTemplate"))
    }

    pub fn relations(&self) -> Vec<Relation> {
        self.get("relations")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(Relation::from_js)
            .collect()
    }

    /// `meta.sheet[fieldId] ?? ""` (`sheetValue`).
    pub fn sheet_value(&self, field: &str) -> String {
        string(self.get("sheet").and_then(|s| s.get(field))).unwrap_or_default()
    }

    /// The sheet's entries in JavaScript's key order.
    pub fn sheet(&self) -> Vec<(String, String)> {
        self.get("sheet")
            .and_then(JsValue::as_object)
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.to_string_lossy(), string(Some(v))?)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `serializeStoryMeta`: one `key: value` line per field.
pub fn serialize_story_meta(meta: Option<&JsValue>) -> String {
    let Some(object) = meta.and_then(JsValue::as_object) else {
        return String::new();
    };
    object
        .iter()
        .map(|(key, value)| format!("{}: {}", key.to_string_lossy(), format_yaml_value(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `parseStoryMeta`: the text of the story data editor back into metadata, or the message for
/// the first bad line.
pub fn parse_story_meta(text: &str) -> Result<Option<JsValue>, String> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut fields = JsObject::new();
    for (index, line) in text.split('\n').enumerate() {
        let n = index + 1;
        let trimmed = line.trim_matches(crate::html::is_js_whitespace);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some(colon) = line.find(':').filter(|c| *c > 0) else {
            return Err(format!("Line {n}: expected \"key: value\"."));
        };
        let key = line[..colon].trim_matches(crate::html::is_js_whitespace);
        if !valid_key(key) {
            return Err(format!("Line {n}: \"{key}\" is not a valid key."));
        }
        let raw = line[colon + 1..].trim_matches(crate::html::is_js_whitespace);
        if raw.is_empty() {
            return Err(format!(
                "Line {n}: missing value. Use [] for an empty list."
            ));
        }
        let Ok(value) = parse_yaml_value(raw) else {
            return Err(format!(
                "Line {n}: invalid value. Use JSON for lists and objects."
            ));
        };
        fields.insert(key, value);
    }
    Ok(normalize_meta(&JsValue::Object(fields)))
}
