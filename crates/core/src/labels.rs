//! Scene and chapter labels (ProseCraft `src/lib/labels.ts` and the page's Labels dialog,
//! upstream `7dae544`).

use crate::edit::EditError;
use crate::js::{JsObject, JsValue};
use crate::project::Label;

/// The eight named colours, usable on light and dark themes.
pub const LABEL_COLORS: [(&str, [u8; 3]); 8] = [
    ("grey", [0x8a, 0x8f, 0x98]),
    ("amber", [0xff, 0xb7, 0x4d]),
    ("green", [0xa8, 0xd5, 0x93]),
    ("blue", [0x7f, 0xb3, 0xff]),
    ("orange", [0xff, 0x8c, 0x42]),
    ("purple", [0xc9, 0xa0, 0xdc]),
    ("red", [0xe4, 0x57, 0x4d]),
    ("pink", [0xd4, 0xa5, 0xff]),
];
pub const MAX_LABELS: usize = 24;
pub const NAME_LIMIT: usize = 40;

/// `DEFAULT_LABELS`.
pub fn default_labels() -> Vec<Label> {
    [
        ("label-draft", "Draft", "grey"),
        ("label-revised", "Revised", "amber"),
        ("label-final", "Final", "green"),
    ]
    .into_iter()
    .map(|(id, name, color)| Label {
        id: id.into(),
        name: name.into(),
        color: color.into(),
    })
    .collect()
}

fn is_hex6(color: &str) -> bool {
    let hex = color.strip_prefix('#').unwrap_or(color);
    hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `labelColorCss` as RGB: a named colour, a `#rrggbb` (or `rrggbb`), else `None` (faint text).
pub fn color_rgb(color: &str) -> Option<[u8; 3]> {
    if let Some((_, rgb)) = LABEL_COLORS.iter().find(|(k, _)| *k == color) {
        return Some(*rgb);
    }
    if !is_hex6(color) {
        return None;
    }
    let hex = color.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// `normalizeLabels`: valid entries (an object's values count as a list), unknown colours
/// grey, first of each id, at most 24; `None` when nothing valid is stored.
pub fn normalize_labels(value: Option<&JsValue>) -> Option<Vec<Label>> {
    let entries: Vec<&JsValue> = match value? {
        JsValue::Array(list) => list.iter().collect(),
        JsValue::Object(object) => object.iter().map(|(_, v)| v).collect(),
        _ => return None,
    };
    let mut valid: Vec<Label> = Vec::new();
    for entry in entries {
        let text = |k: &str| match entry.get(k) {
            Some(JsValue::String(s)) => Some(s.to_string_lossy()),
            _ => None,
        };
        let (Some(id), Some(name), Some(color)) = (text("id"), text("name"), text("color")) else {
            continue;
        };
        if id.is_empty() || valid.iter().any(|l| l.id == id) {
            continue;
        }
        let known = LABEL_COLORS.iter().any(|(k, _)| *k == color);
        let color = if known || is_hex6(&color) {
            color
        } else {
            "grey".into()
        };
        valid.push(Label { id, name, color });
        if valid.len() >= MAX_LABELS {
            break;
        }
    }
    (!valid.is_empty()).then_some(valid)
}

/// `projectLabels`: the stored labels, or the defaults.
pub fn project_labels(project: &JsValue) -> Vec<Label> {
    normalize_labels(project.get("labels")).unwrap_or_else(default_labels)
}

fn clear_label(project: &mut JsValue, key: &str, label: &str) {
    if let Some(JsValue::Array(items)) = project.as_object_mut().and_then(|o| o.get_mut(key)) {
        for item in items.iter_mut() {
            let matches = item
                .get("labelId")
                .and_then(JsValue::as_js_string)
                .is_some_and(|l| l.eq_str(label));
            if matches && let Some(o) = item.as_object_mut() {
                o.remove("labelId");
            }
        }
    }
}

/// `saveLabels`: names trimmed (a blank one saves nothing: `Err(EditError::Shape)`); labels
/// no longer there are cleared from scenes and chapters.
pub fn save_labels(project: &mut JsValue, labels: &[Label]) -> Result<(), EditError> {
    let next: Vec<Label> = labels
        .iter()
        .map(|l| Label {
            name: l
                .name
                .trim_matches(crate::html::is_js_whitespace)
                .to_owned(),
            ..l.clone()
        })
        .collect();
    if next.iter().any(|l| l.name.is_empty()) {
        return Err(EditError::Shape);
    }
    for gone in project_labels(project)
        .iter()
        .filter(|old| !next.iter().any(|l| l.id == old.id))
    {
        clear_label(project, "documents", &gone.id);
        clear_label(project, "chapters", &gone.id);
    }
    let list = next
        .iter()
        .map(|l| {
            let mut o = JsObject::new();
            o.insert("id", JsValue::str(&l.id));
            o.insert("name", JsValue::str(&l.name));
            o.insert("color", JsValue::str(&l.color));
            JsValue::Object(o)
        })
        .collect();
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("labels", JsValue::Array(list));
    Ok(())
}

/// `applyLabel`: a scene's or a chapter's label, or none.
pub fn set_label(
    project: &mut JsValue,
    chapter: bool,
    id: &str,
    label: Option<&str>,
) -> Result<(), EditError> {
    let key = if chapter { "chapters" } else { "documents" };
    let Some(JsValue::Array(items)) = project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .get_mut(key)
    else {
        return Err(EditError::Shape);
    };
    let item = items
        .iter_mut()
        .find(|i| {
            i.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|v| v.eq_str(id))
        })
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?;
    match label {
        Some(label) => item.insert("labelId", JsValue::str(label)),
        None => {
            item.remove("labelId");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        color_rgb, default_labels, normalize_labels, project_labels, save_labels, set_label,
    };
    use crate::js::{parse, stringify};
    use crate::project::Label;

    #[test]
    fn stored_labels_are_normalized_or_default() {
        let p = parse(r##"{"labels":[{"id":"a","name":"A","color":"teal"},{"id":"a","name":"dup","color":"red"},{"id":"b","name":"B","color":"#00ff00"},{"id":"","name":"x","color":"red"},{"name":"no id","color":"red"}]}"##).expect("json");
        let labels = project_labels(&p);
        assert_eq!(
            labels
                .iter()
                .map(|l| (l.id.as_str(), l.color.as_str()))
                .collect::<Vec<_>>(),
            [("a", "grey"), ("b", "#00ff00")]
        );
        assert_eq!(
            project_labels(&parse("{}").expect("json")),
            default_labels()
        );
        assert_eq!(
            normalize_labels(Some(
                &parse(r#"{"x":{"id":"o","name":"O","color":"red"}}"#).expect("json")
            ))
            .map(|l| l.len()),
            Some(1)
        );
        assert_eq!(color_rgb("#00ff00"), Some([0, 255, 0]));
        assert_eq!(color_rgb("amber"), Some([0xff, 0xb7, 0x4d]));
        assert_eq!(color_rgb("teal"), None);
    }

    #[test]
    fn saving_clears_removed_labels_from_scenes_and_chapters() {
        let mut p = parse(r#"{"documents":[{"id":"s","labelId":"label-final"},{"id":"t","labelId":"label-draft"}],"chapters":[{"id":"c","labelId":"label-final"}]}"#).expect("json");
        let mut labels = default_labels();
        labels.retain(|l| l.id != "label-final");
        labels[0].name = "  Rough ".into();
        labels.push(Label {
            id: "label-x".into(),
            name: "Polish".into(),
            color: "blue".into(),
        });
        save_labels(&mut p, &labels).expect("save");
        assert_eq!(
            stringify(&p),
            r#"{"documents":[{"id":"s"},{"id":"t","labelId":"label-draft"}],"chapters":[{"id":"c"}],"labels":[{"id":"label-draft","name":"Rough","color":"grey"},{"id":"label-revised","name":"Revised","color":"amber"},{"id":"label-x","name":"Polish","color":"blue"}]}"#
        );
        labels[0].name = " ".into();
        assert!(
            save_labels(&mut p, &labels).is_err(),
            "a blank name saves nothing"
        );
        set_label(&mut p, true, "c", Some("label-x")).expect("set");
        set_label(&mut p, false, "t", None).expect("clear");
        assert!(stringify(&p).starts_with(
            r#"{"documents":[{"id":"s"},{"id":"t"}],"chapters":[{"id":"c","labelId":"label-x"}]"#
        ));
    }
}
