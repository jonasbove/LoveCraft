//! Changes the network view, the character sheet and the story data panel make to a project
//! (ProseCraft `+page.svelte`'s `updateSceneMeta`, `confirmSceneCharacter` and
//! `dismissRelationSuggestion`, and the relation and template helpers, upstream `bcb884d`).

use crate::edit::EditError;
use crate::js::{JsObject, JsValue};

use super::meta::Relation;
use super::templates::{Template, to_js_list};

fn documents_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    match project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .get_mut("documents")
    {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn is(doc: &JsValue, id: &str) -> bool {
    doc.get("id")
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

/// `updateSceneMeta`: a document's `meta` replaced; an empty or missing one removes the key.
pub fn set_meta(
    project: &mut JsValue,
    document: &str,
    meta: Option<JsValue>,
) -> Result<(), EditError> {
    let doc = documents_mut(project)?
        .iter_mut()
        .find(|d| is(d, document))
        .ok_or_else(|| EditError::NoDocument(document.into()))?;
    let object = doc.as_object_mut().ok_or(EditError::Shape)?;
    match meta.filter(|m| m.as_object().is_some_and(|o| !o.is_empty())) {
        Some(meta) => object.insert("meta", meta),
        None => {
            object.remove("meta");
        }
    }
    Ok(())
}

/// `confirmSceneCharacter`: a detected page added to a scene's confirmed characters.
pub fn confirm_character(project: &mut JsValue, scene: &str, page: &str) -> Result<(), EditError> {
    let doc = documents_mut(project)?
        .iter()
        .find(|d| is(d, scene))
        .ok_or_else(|| EditError::NoDocument(scene.into()))?;
    let mut meta = doc
        .get("meta")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut characters = meta
        .get("characters")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    if characters
        .iter()
        .any(|c| c.as_js_string().is_some_and(|c| c.eq_str(page)))
    {
        return Ok(());
    }
    characters.push(JsValue::str(page));
    meta.insert("characters", JsValue::Array(characters));
    set_meta(project, scene, Some(JsValue::Object(meta)))
}

/// `addRelation` with the id the caller drew.
pub fn add_relation(
    project: &mut JsValue,
    source: &str,
    relation: &Relation,
) -> Result<(), EditError> {
    super::relations::add_relation(documents_mut(project)?, source, relation.clone());
    Ok(())
}

/// `updateRelation`: each `(key, value)` set on the relation, `None` removing the key (an
/// `undefined` in ProseCraft's patch).
pub fn update_relation(
    project: &mut JsValue,
    source: &str,
    relation: &str,
    patch: &[(String, Option<JsValue>)],
) -> Result<(), EditError> {
    super::relations::update_relation(
        documents_mut(project)?,
        source,
        relation,
        |o: &mut JsObject| {
            for (key, value) in patch {
                match value {
                    Some(v) => o.insert(key.as_str(), v.clone()),
                    None => {
                        o.remove(key);
                    }
                }
            }
        },
    );
    Ok(())
}

pub fn remove_relation(
    project: &mut JsValue,
    source: &str,
    relation: &str,
) -> Result<(), EditError> {
    super::relations::remove_relation(documents_mut(project)?, source, relation);
    Ok(())
}

/// The project's own sheet templates (`sheetTemplates`).
pub fn set_sheet_templates(project: &mut JsValue, templates: &[Template]) -> Result<(), EditError> {
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("sheetTemplates", to_js_list(templates));
    Ok(())
}

/// `dismissRelationSuggestion`: the pair's key remembered so the suggestion stays away.
pub fn dismiss_suggestion(project: &mut JsValue, key: &str) -> Result<(), EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    let mut list = object
        .get("dismissedSuggestions")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    if list
        .iter()
        .any(|k| k.as_js_string().is_some_and(|k| k.eq_str(key)))
    {
        return Ok(());
    }
    list.push(JsValue::str(key));
    object.insert("dismissedSuggestions", JsValue::Array(list));
    Ok(())
}

/// The project's dismissed suggestion keys.
pub fn dismissed(project: &JsValue) -> Vec<String> {
    project
        .get("dismissedSuggestions")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(JsValue::as_js_string)
        .map(crate::js::JsString::to_string_lossy)
        .collect()
}
