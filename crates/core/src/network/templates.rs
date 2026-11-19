//! Character-sheet templates (ProseCraft `src/lib/sheet-templates.ts`, upstream `bcb884d`): the
//! built-in presets, the author's own (stored as the project's `sheetTemplates`) and applying
//! one to a page. Applying never deletes a value: fields a template doesn't define stay in
//! `meta.sheet` and are reported as orphans.

use crate::js::{JsObject, JsString, JsValue};

pub const MAX_TEMPLATES: usize = 20;
pub const MAX_TEMPLATE_FIELDS: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub id: String,
    pub label: String,
    /// `text`, `longtext`, `number`, `choice` or `link`.
    pub kind: String,
    pub options: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub fields: Vec<Field>,
}

fn field(id: &str, label: &str, kind: &str) -> Field {
    Field {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        options: None,
    }
}

fn common() -> Vec<Field> {
    vec![
        field("role", "Role", "text"),
        field("age", "Age", "number"),
        field("appearance", "Appearance", "longtext"),
        field("voice", "Voice", "longtext"),
        field("goals", "Goals", "longtext"),
        field("flaws", "Flaws", "longtext"),
        field("secret", "Secret", "longtext"),
    ]
}

/// THRILLER, FANTASY, ROMANCE and BLANK.
pub fn built_in() -> Vec<Template> {
    let mut fantasy = common();
    fantasy.extend([
        field("languages", "Languages", "text"),
        field("birthplace", "Birthplace", "link"),
        field("occupation", "Occupation", "text"),
        field("abilities", "Abilities", "longtext"),
    ]);
    let mut romance = common();
    romance.extend([
        field("occupation", "Occupation", "text"),
        field("birthplace", "Birthplace", "link"),
    ]);
    vec![
        Template {
            id: "thriller".into(),
            name: "Thriller".into(),
            fields: common(),
        },
        Template {
            id: "fantasy".into(),
            name: "Fantasy".into(),
            fields: fantasy,
        },
        Template {
            id: "romance".into(),
            name: "Romance".into(),
            fields: romance,
        },
        Template {
            id: "blank".into(),
            name: "Blank".into(),
            fields: Vec::new(),
        },
    ]
}

fn text(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

impl Field {
    fn from_js(value: &JsValue) -> Self {
        Self {
            id: text(value, "id"),
            label: text(value, "label"),
            kind: text(value, "type"),
            options: value.get("options").and_then(JsValue::as_array).map(|o| {
                o.iter()
                    .filter_map(JsValue::as_js_string)
                    .map(JsString::to_string_lossy)
                    .collect()
            }),
        }
    }

    fn to_js(&self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("id", JsValue::str(&self.id));
        o.insert("label", JsValue::str(&self.label));
        o.insert("type", JsValue::str(&self.kind));
        if let Some(options) = &self.options {
            o.insert(
                "options",
                JsValue::Array(options.iter().map(|s| JsValue::str(s)).collect()),
            );
        }
        JsValue::Object(o)
    }
}

impl Template {
    pub fn from_js(value: &JsValue) -> Self {
        Self {
            id: text(value, "id"),
            name: text(value, "name"),
            fields: value
                .get("fields")
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .iter()
                .map(Field::from_js)
                .collect(),
        }
    }

    pub fn to_js(&self) -> JsValue {
        let mut o = JsObject::new();
        o.insert("id", JsValue::str(&self.id));
        o.insert("name", JsValue::str(&self.name));
        o.insert(
            "fields",
            JsValue::Array(self.fields.iter().map(Field::to_js).collect()),
        );
        JsValue::Object(o)
    }
}

/// The project's own templates (`sheetTemplates`).
pub fn project_templates(project: &JsValue) -> Vec<Template> {
    project
        .get("sheetTemplates")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .map(Template::from_js)
        .collect()
}

pub fn to_js_list(templates: &[Template]) -> JsValue {
    JsValue::Array(templates.iter().map(Template::to_js).collect())
}

/// `allTemplates`: built-ins, then the project's.
pub fn all_templates(project: &JsValue) -> Vec<Template> {
    let mut all = built_in();
    all.extend(project_templates(project));
    all
}

/// `applyTemplate`: the sheet reordered to the template's fields, the rest kept after them;
/// returns the new meta and the orphaned field ids.
pub fn apply_template(meta: Option<&JsValue>, template: &Template) -> (JsValue, Vec<String>) {
    let mut next = meta
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let sheet = next
        .get("sheet")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut entries = JsObject::new();
    for f in &template.fields {
        if let Some(value) = sheet.get(&f.id) {
            entries.insert(f.id.as_str(), value.clone());
        }
    }
    let mut orphans = Vec::new();
    for (key, value) in sheet.iter() {
        let name = key.to_string_lossy();
        if template.fields.iter().any(|f| f.id == name) {
            continue;
        }
        entries.insert(key.clone(), value.clone());
        orphans.push(name);
    }
    if entries.is_empty() {
        next.remove("sheet");
    } else {
        next.insert("sheet", JsValue::Object(entries));
    }
    (JsValue::Object(next), orphans)
}

/// `setSheetValue`: an empty value removes the entry; an empty sheet (and meta) goes.
pub fn set_sheet_value(meta: Option<&JsValue>, field_id: &str, value: &str) -> Option<JsValue> {
    if field_id.is_empty() {
        return meta.cloned();
    }
    let mut next = meta
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut sheet = next
        .get("sheet")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    if value.is_empty() {
        sheet.remove(field_id);
    } else {
        sheet.insert(field_id, JsValue::str(value));
    }
    if sheet.is_empty() {
        next.remove("sheet");
    } else {
        next.insert("sheet", JsValue::Object(sheet));
    }
    (!next.is_empty()).then_some(JsValue::Object(next))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueError {
    Number,
    Choice,
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(crate::html::is_js_whitespace)
}

/// `^[+-]?(?:\d+(?:\.\d+)?|\.\d+)$`.
fn number_value(text: &str) -> bool {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match body.split_once('.') {
        None => digits(body),
        Some((whole, fraction)) => (whole.is_empty() || digits(whole)) && digits(fraction),
    }
}

/// `validateSheetValue`: empty is always fine; numbers and choices are checked.
pub fn validate_sheet_value(field: &Field, value: &str) -> Option<ValueError> {
    let text = js_trim(value);
    if text.is_empty() {
        return None;
    }
    match field.kind.as_str() {
        "number" if !number_value(text) => Some(ValueError::Number),
        "choice"
            if !field
                .options
                .as_deref()
                .unwrap_or_default()
                .iter()
                .any(|o| o == text) =>
        {
            Some(ValueError::Choice)
        }
        _ => None,
    }
}

fn clean_options(options: Option<&[String]>) -> Option<Vec<String>> {
    let options = options?;
    let mut cleaned: Vec<String> = Vec::new();
    for option in options {
        let text = js_trim(option);
        if !text.is_empty() && !cleaned.iter().any(|c| c == text) {
            cleaned.push(text.into());
        }
    }
    (!cleaned.is_empty()).then_some(cleaned)
}

fn update(templates: &mut [Template], id: &str, f: impl FnOnce(&mut Template)) {
    if let Some(t) = templates.iter_mut().find(|t| t.id == id) {
        f(t);
    }
}

/// `addTemplate`: an empty template, unless there are already 20.
pub fn add_template(templates: &mut Vec<Template>, name: &str, id: impl FnOnce() -> String) {
    if templates.len() >= MAX_TEMPLATES {
        return;
    }
    let name = js_trim(name);
    templates.push(Template {
        id: id(),
        name: if name.is_empty() {
            "Untitled template".into()
        } else {
            name.into()
        },
        fields: Vec::new(),
    });
}

/// `renameTemplate`: blank names are ignored.
pub fn rename_template(templates: &mut [Template], id: &str, name: &str) {
    let name = js_trim(name);
    if !name.is_empty() {
        update(templates, id, |t| t.name = name.into());
    }
}

pub fn remove_template(templates: &mut Vec<Template>, id: &str) {
    templates.retain(|t| t.id != id);
}

/// `addField`: at most 40 fields, a label required; options only on a choice field.
pub fn add_field(
    templates: &mut [Template],
    template_id: &str,
    label: &str,
    kind: &str,
    options: Option<&[String]>,
    id: impl FnOnce() -> String,
) {
    update(templates, template_id, |t| {
        let label = js_trim(label);
        if t.fields.len() >= MAX_TEMPLATE_FIELDS || label.is_empty() {
            return;
        }
        t.fields.push(Field {
            id: id(),
            label: label.into(),
            kind: kind.into(),
            options: if kind == "choice" {
                clean_options(options)
            } else {
                None
            },
        });
    });
}

pub fn rename_field(templates: &mut [Template], template_id: &str, field_id: &str, label: &str) {
    let label = js_trim(label);
    if label.is_empty() {
        return;
    }
    update(templates, template_id, |t| {
        for f in t.fields.iter_mut().filter(|f| f.id == field_id) {
            f.label = label.into();
        }
    });
}

/// `reorderField`: to an index clamped to the list.
pub fn reorder_field(templates: &mut [Template], template_id: &str, field_id: &str, to: f64) {
    if !to.is_finite() {
        return;
    }
    update(templates, template_id, |t| {
        let Some(from) = t.fields.iter().position(|f| f.id == field_id) else {
            return;
        };
        let last = t.fields.len().saturating_sub(1) as f64;
        let to = to.trunc().clamp(0.0, last) as usize;
        if from != to {
            let moved = t.fields.remove(from);
            t.fields.insert(to, moved);
        }
    });
}

pub fn remove_field(templates: &mut [Template], template_id: &str, field_id: &str) {
    update(templates, template_id, |t| {
        t.fields.retain(|f| f.id != field_id)
    });
}

/// `changeFieldType`: options only survive on a choice field.
pub fn change_field_type(
    templates: &mut [Template],
    template_id: &str,
    field_id: &str,
    kind: &str,
) {
    update(templates, template_id, |t| {
        for f in t.fields.iter_mut().filter(|f| f.id == field_id) {
            f.kind = kind.into();
            if kind != "choice" {
                f.options = None;
            }
        }
    });
}

pub fn set_field_options(
    templates: &mut [Template],
    template_id: &str,
    field_id: &str,
    options: &[String],
) {
    update(templates, template_id, |t| {
        for f in t.fields.iter_mut().filter(|f| f.id == field_id) {
            f.options = clean_options(Some(options));
        }
    });
}

/// `uniqueTemplateName`: the name, else "<name> copy", "<name> copy 2", ….
pub fn unique_template_name(templates: &[Template], name: &str) -> String {
    let base = match js_trim(name) {
        "" => "Untitled template",
        n => n,
    };
    let taken: Vec<String> = templates
        .iter()
        .map(|t| js_trim(&t.name).to_lowercase())
        .collect();
    let free = |c: &str| !taken.contains(&c.to_lowercase());
    if free(base) {
        return base.into();
    }
    let mut candidate = format!("{base} copy");
    let mut index = 2;
    while !free(&candidate) {
        candidate = format!("{base} copy {index}");
        index += 1;
    }
    candidate
}
