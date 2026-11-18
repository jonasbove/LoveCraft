//! ProseCraft's eight citation styles and its English and German terms
//! (`src/lib/citations/styles.ts`, upstream `7dae544`), embedded as the data ProseCraft's
//! fixture generator exported (`styles.json`) rather than retyped.

use crate::js::{self, JsObject, JsValue};
use std::sync::OnceLock;

/// A style's id, its name as the publish settings show it, and its kind
/// (`author-date`, `numeric` or `note`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleMeta {
    pub id: String,
    pub label: String,
    pub kind: String,
}

fn data() -> &'static JsValue {
    static DATA: OnceLock<JsValue> = OnceLock::new();
    DATA.get_or_init(|| js::parse(include_str!("styles.json")).unwrap_or(JsValue::Null))
}

/// `STYLES`: the built-in styles, in ProseCraft's order.
pub fn styles() -> Vec<StyleMeta> {
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    data()
        .get("styles")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .map(|s| StyleMeta {
            id: text(s, "id"),
            label: text(s, "label"),
            kind: text(s, "kind"),
        })
        .collect()
}

/// `isValidStyleId`.
pub fn is_valid_style_id(id: &str) -> bool {
    styles().iter().any(|s| s.id == id)
}

/// A style's settings (`StyleTemplate`): its in-text, bibliography and name rules.
pub struct Template {
    in_text: JsObject,
    bibliography: JsObject,
    names: JsObject,
}

fn object(value: Option<&JsValue>) -> JsObject {
    value
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default()
}

fn text_of(object: &JsObject, key: &str) -> String {
    object
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

fn number_of(object: &JsObject, key: &str) -> f64 {
    object
        .get(key)
        .and_then(JsValue::as_number)
        .unwrap_or(f64::NAN)
}

fn flag_of(object: &JsObject, key: &str) -> bool {
    matches!(object.get(key), Some(JsValue::Bool(true)))
}

impl Template {
    pub fn in_text_s(&self, key: &str) -> String {
        text_of(&self.in_text, key)
    }
    pub fn in_text_n(&self, key: &str) -> f64 {
        number_of(&self.in_text, key)
    }
    pub fn in_text_b(&self, key: &str) -> bool {
        flag_of(&self.in_text, key)
    }
    pub fn bib_s(&self, key: &str) -> String {
        text_of(&self.bibliography, key)
    }
    pub fn names_s(&self, key: &str) -> String {
        text_of(&self.names, key)
    }
    pub fn names_n(&self, key: &str) -> f64 {
        number_of(&self.names, key)
    }
    pub fn names_b(&self, key: &str) -> bool {
        flag_of(&self.names, key)
    }
}

/// `getStyleTemplate` (APA 7 for an id that isn't one of the eight).
pub fn template(style: &str) -> Template {
    let templates = data().get("templates");
    let template = templates
        .and_then(|t| t.get(style))
        .or_else(|| templates.and_then(|t| t.get("apa7")));
    let bibliography = object(template.and_then(|t| t.get("bibliography")));
    Template {
        in_text: object(template.and_then(|t| t.get("inText"))),
        names: object(bibliography.get("names")),
        bibliography,
    }
}

/// A locale's words (`LocaleTerms`).
pub struct Terms(JsObject);

impl Terms {
    pub fn get(&self, key: &str) -> String {
        text_of(&self.0, key)
    }
    pub fn b(&self, key: &str) -> bool {
        flag_of(&self.0, key)
    }
}

/// `LOCALE_TERMS[locale]` (English for a locale that isn't `de`).
pub fn terms(locale: &str) -> Terms {
    let all = data().get("terms");
    Terms(object(
        all.and_then(|t| t.get(locale))
            .or_else(|| all.and_then(|t| t.get("en"))),
    ))
}
