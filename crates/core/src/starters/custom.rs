//! The user's own wiki templates (ProseCraft `src/lib/starters.ts`, `CustomStarter`, upstream
//! `7dae544`): blueprints composed in the Start-from dialog and saved on this device, and the
//! custom folders and pages they are made of.

use super::{Blueprint, BlueprintFolder, BlueprintPage, all_keys, module_folder_key, modules};
use crate::html::{is_js_whitespace, sanitize_html};
use crate::js::{self, JsObject, JsValue};

/// The device setting the templates are kept in.
pub const CUSTOM_STARTERS_KEY: &str = "prosecraft-custom-starters";
const MAX_CUSTOM_STARTERS: usize = 20;
const MAX_CUSTOM_FOLDERS: usize = 40;
const MAX_CUSTOM_PAGES: usize = 200;
const MAX_NAME_LENGTH: usize = 120;
const MAX_HTML_LENGTH: usize = 50_000;

/// A template the user composed and saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomStarter {
    pub id: String,
    pub name: String,
    pub blueprint: Blueprint,
}

fn units(s: &str, max: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().take(max).collect();
    String::from_utf16_lossy(&u)
}

/// `text`: a trimmed name (120 at most), or `fallback`.
fn text(value: Option<&JsValue>, fallback: &str) -> String {
    match value {
        Some(JsValue::String(s)) => {
            let s = s.to_string_lossy();
            let trimmed = s.trim_matches(is_js_whitespace);
            if trimmed.is_empty() {
                fallback.to_owned()
            } else {
                units(trimmed, MAX_NAME_LENGTH)
            }
        }
        _ => fallback.to_owned(),
    }
}

fn string(value: Option<&JsValue>) -> Option<String> {
    match value {
        Some(JsValue::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

/// `storedModuleId`: the module a stored folder key names, when it is still a known module.
fn stored_module_id(key: Option<&JsValue>) -> Option<String> {
    let id = string(key)?.strip_prefix("module:")?.to_owned();
    modules().iter().any(|m| m.id == id).then_some(id)
}

/// `normalizeCustomStarters`: good entries kept, bad ones dropped, keys made again.
pub fn normalize_custom_starters(value: &JsValue) -> Vec<CustomStarter> {
    let Some(entries) = value.as_array() else {
        return Vec::new();
    };
    let mut result: Vec<CustomStarter> = Vec::new();
    for entry in entries {
        if result.len() >= MAX_CUSTOM_STARTERS {
            break;
        }
        if !matches!(entry, JsValue::Object(_)) {
            continue;
        }
        let Some(id) = string(entry.get("id")).filter(|i| !i.is_empty()) else {
            continue;
        };
        if result.iter().any(|s| s.id == id) {
            continue;
        }
        let Some(raw_folders) = entry
            .get("blueprint")
            .and_then(|b| b.get("folders"))
            .and_then(JsValue::as_array)
        else {
            continue;
        };
        let mut page_count = 0;
        let mut folders: Vec<BlueprintFolder> = Vec::new();
        for raw_folder in raw_folders.iter().take(MAX_CUSTOM_FOLDERS) {
            if !matches!(raw_folder, JsValue::Object(_)) {
                continue;
            }
            let folder_index = folders.len();
            // Module folders keep their key, so a saved template still reads as the same module.
            let module_key = stored_module_id(raw_folder.get("key"))
                .map(|id| module_folder_key(&id))
                .filter(|k| !folders.iter().any(|f| f.key == *k));
            let raw_pages = raw_folder
                .get("pages")
                .and_then(JsValue::as_array)
                .unwrap_or_default();
            let mut pages = Vec::new();
            for raw_page in raw_pages {
                if page_count >= MAX_CUSTOM_PAGES {
                    break;
                }
                if !matches!(raw_page, JsValue::Object(_)) {
                    continue;
                }
                let html = string(raw_page.get("html"))
                    .map(|h| sanitize_html(&units(&h, MAX_HTML_LENGTH)))
                    .unwrap_or_default();
                let key = match &module_key {
                    Some(m) => format!("{m}/{}", pages.len()),
                    None => format!("page:{folder_index}.{}", pages.len()),
                };
                pages.push(BlueprintPage {
                    key,
                    title: text(raw_page.get("title"), "Untitled"),
                    html,
                });
                page_count += 1;
            }
            folders.push(BlueprintFolder {
                key: module_key.unwrap_or_else(|| format!("folder:{folder_index}")),
                name: text(raw_folder.get("name"), "Untitled"),
                pages,
            });
        }
        result.push(CustomStarter {
            id,
            name: text(entry.get("name"), "My template"),
            blueprint: Blueprint { folders },
        });
    }
    result
}

/// The templates as ProseCraft writes them.
pub fn custom_starters_value(starters: &[CustomStarter]) -> JsValue {
    JsValue::Array(
        starters
            .iter()
            .map(|s| {
                let folders = s
                    .blueprint
                    .folders
                    .iter()
                    .map(|f| {
                        let pages = f
                            .pages
                            .iter()
                            .map(|p| {
                                let mut o = JsObject::new();
                                o.insert("key", JsValue::str(&p.key));
                                o.insert("title", JsValue::str(&p.title));
                                o.insert("html", JsValue::str(&p.html));
                                JsValue::Object(o)
                            })
                            .collect();
                        let mut o = JsObject::new();
                        o.insert("key", JsValue::str(&f.key));
                        o.insert("name", JsValue::str(&f.name));
                        o.insert("pages", JsValue::Array(pages));
                        JsValue::Object(o)
                    })
                    .collect();
                let mut blueprint = JsObject::new();
                blueprint.insert("folders", JsValue::Array(folders));
                let mut o = JsObject::new();
                o.insert("id", JsValue::str(&s.id));
                o.insert("name", JsValue::str(&s.name));
                o.insert("blueprint", JsValue::Object(blueprint));
                JsValue::Object(o)
            })
            .collect(),
    )
}

/// `loadCustomStarters`: the saved templates (none when unreadable).
pub fn load_custom_starters(saved: Option<&str>) -> Vec<CustomStarter> {
    saved
        .and_then(|t| js::parse(t).ok())
        .map(|v| normalize_custom_starters(&v))
        .unwrap_or_default()
}

/// `saveCustomStarters`: what the setting holds (the templates normalized again).
pub fn custom_starters_json(starters: &[CustomStarter]) -> String {
    js::stringify(&custom_starters_value(&normalize_custom_starters(
        &custom_starters_value(starters),
    )))
}

/// `freshKey`: the first `prefix:cN` the blueprint doesn't use.
fn fresh_key(blueprint: &Blueprint, prefix: &str) -> String {
    let used = all_keys(blueprint);
    (0..)
        .map(|n| format!("{prefix}:c{n}"))
        .find(|k| !used.contains(k))
        .unwrap_or_default()
}

fn trimmed_or(value: &str, fallback: &str) -> String {
    let t = value.trim_matches(is_js_whitespace);
    if t.is_empty() {
        fallback.to_owned()
    } else {
        t.to_owned()
    }
}

/// `addCustomFolder`.
pub fn add_custom_folder(blueprint: &Blueprint, name: &str) -> Blueprint {
    let mut next = blueprint.clone();
    next.folders.push(BlueprintFolder {
        key: fresh_key(blueprint, "folder"),
        name: trimmed_or(name, "New folder"),
        pages: Vec::new(),
    });
    next
}

/// `addCustomPage`.
pub fn add_custom_page(
    blueprint: &Blueprint,
    folder_key: &str,
    title: &str,
    html: &str,
) -> Blueprint {
    let key = fresh_key(blueprint, "page");
    let mut next = blueprint.clone();
    for folder in next.folders.iter_mut().filter(|f| f.key == folder_key) {
        folder.pages.push(BlueprintPage {
            key: key.clone(),
            title: trimmed_or(title, "New page"),
            html: html.to_owned(),
        });
    }
    next
}

/// `removeCustomFolder`.
pub fn remove_custom_folder(blueprint: &Blueprint, folder_key: &str) -> Blueprint {
    Blueprint {
        folders: blueprint
            .folders
            .iter()
            .filter(|f| f.key != folder_key)
            .cloned()
            .collect(),
    }
}

/// `removeCustomPage`.
pub fn remove_custom_page(blueprint: &Blueprint, page_key: &str) -> Blueprint {
    let mut next = blueprint.clone();
    for folder in &mut next.folders {
        folder.pages.retain(|p| p.key != page_key);
    }
    next
}

/// `renameCustomFolder`: a blank name keeps the old one.
pub fn rename_custom_folder(blueprint: &Blueprint, folder_key: &str, name: &str) -> Blueprint {
    let mut next = blueprint.clone();
    for folder in next.folders.iter_mut().filter(|f| f.key == folder_key) {
        folder.name = trimmed_or(name, &folder.name);
    }
    next
}

/// `renameCustomPage`: a blank title keeps the old one.
pub fn rename_custom_page(blueprint: &Blueprint, page_key: &str, title: &str) -> Blueprint {
    let mut next = blueprint.clone();
    for page in next
        .folders
        .iter_mut()
        .flat_map(|f| f.pages.iter_mut())
        .filter(|p| p.key == page_key)
    {
        page.title = trimmed_or(title, &page.title);
    }
    next
}
