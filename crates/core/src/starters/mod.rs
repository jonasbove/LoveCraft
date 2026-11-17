//! New projects and their wiki starters (ProseCraft `src/lib/starters.ts` and the page's
//! `createProjectWithStarter`, upstream `7dae544`).
//!
//! The starters' content (presets, world-building modules and their template pages) and the
//! default publishing options are ProseCraft's own, exported from its code into `data.json`
//! by the fixture generator, so the templates are exactly ProseCraft's.

pub mod custom;

use std::sync::OnceLock;

use crate::js::{self, JsObject, JsValue};

/// A preset: id, label, description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Starter {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// A world-building module (Characters, Places, Magic & Technology…).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: String,
    pub label: String,
    pub description: String,
    pub folder: String,
    pub pages: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintPage {
    pub key: String,
    pub title: String,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintFolder {
    pub key: String,
    pub name: String,
    pub pages: Vec<BlueprintPage>,
}

/// The wiki a new project starts with; keys are unique within one blueprint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Blueprint {
    pub folders: Vec<BlueprintFolder>,
}

struct Data {
    starters: Vec<Starter>,
    blueprints: Vec<(String, Blueprint)>,
    modules: Vec<Module>,
    publish_options: JsValue,
}

fn s(v: &JsValue, key: &str) -> String {
    v.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// A blueprint as ProseCraft writes it (`{ folders: [{ key, name, pages: [{ key, title, html }] }] }`).
pub fn read_blueprint(v: &JsValue) -> Blueprint {
    let folders = v
        .get("folders")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    Blueprint {
        folders: folders
            .iter()
            .map(|f| BlueprintFolder {
                key: s(f, "key"),
                name: s(f, "name"),
                pages: f
                    .get("pages")
                    .and_then(JsValue::as_array)
                    .unwrap_or_default()
                    .iter()
                    .map(|p| BlueprintPage {
                        key: s(p, "key"),
                        title: s(p, "title"),
                        html: s(p, "html"),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(|| {
        let value = js::parse(include_str!("data.json")).unwrap_or(JsValue::Null);
        let list = |k: &str| {
            value
                .get(k)
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .to_vec()
        };
        Data {
            starters: list("starters")
                .iter()
                .map(|x| Starter {
                    id: s(x, "id"),
                    label: s(x, "label"),
                    description: s(x, "description"),
                })
                .collect(),
            blueprints: value
                .get("blueprints")
                .and_then(JsValue::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.to_string_lossy(), read_blueprint(v)))
                        .collect()
                })
                .unwrap_or_default(),
            modules: list("modules")
                .iter()
                .map(|m| Module {
                    id: s(m, "id"),
                    label: s(m, "label"),
                    description: s(m, "description"),
                    folder: s(m, "folder"),
                    pages: m
                        .get("pages")
                        .and_then(JsValue::as_array)
                        .unwrap_or_default()
                        .iter()
                        .map(|p| (s(p, "title"), s(p, "html")))
                        .collect(),
                })
                .collect(),
            publish_options: value
                .get("defaultPublishOptions")
                .cloned()
                .unwrap_or(JsValue::Object(JsObject::new())),
        }
    })
}

/// The presets, in ProseCraft's order (Blank first).
pub fn starters() -> &'static [Starter] {
    &data().starters
}

/// The world-building modules.
pub fn modules() -> &'static [Module] {
    &data().modules
}

/// `starterBlueprint`: a preset as an editable blueprint.
pub fn starter_blueprint(id: &str) -> Blueprint {
    data()
        .blueprints
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, b)| b.clone())
        .unwrap_or_default()
}

/// `moduleFolderKey`.
pub fn module_folder_key(id: &str) -> String {
    format!("module:{id}")
}

/// `blueprintHasModule`.
pub fn has_module(blueprint: &Blueprint, id: &str) -> bool {
    let key = module_folder_key(id);
    blueprint.folders.iter().any(|f| f.key == key)
}

/// `addModule`: appends the module's folder with its pages; nothing when present or unknown.
pub fn add_module(blueprint: &Blueprint, id: &str) -> Blueprint {
    let Some(module) = modules().iter().find(|m| m.id == id) else {
        return blueprint.clone();
    };
    if has_module(blueprint, id) {
        return blueprint.clone();
    }
    let key = module_folder_key(id);
    let mut out = blueprint.clone();
    out.folders.push(BlueprintFolder {
        key: key.clone(),
        name: module.folder.clone(),
        pages: module
            .pages
            .iter()
            .enumerate()
            .map(|(i, (title, html))| BlueprintPage {
                key: format!("{key}/{i}"),
                title: title.clone(),
                html: html.clone(),
            })
            .collect(),
    });
    out
}

/// `removeModule`.
pub fn remove_module(blueprint: &Blueprint, id: &str) -> Blueprint {
    let key = module_folder_key(id);
    Blueprint {
        folders: blueprint
            .folders
            .iter()
            .filter(|f| f.key != key)
            .cloned()
            .collect(),
    }
}

/// `allBlueprintKeys`, in order.
pub fn all_keys(blueprint: &Blueprint) -> Vec<String> {
    blueprint
        .folders
        .iter()
        .flat_map(|f| std::iter::once(f.key.clone()).chain(f.pages.iter().map(|p| p.key.clone())))
        .collect()
}

/// `filterBlueprint`: the selected pages; a folder stays when it or any of its pages is
/// selected.
pub fn filter_blueprint(blueprint: &Blueprint, selected: &[String]) -> Blueprint {
    Blueprint {
        folders: blueprint
            .folders
            .iter()
            .filter_map(|f| {
                let pages: Vec<BlueprintPage> = f
                    .pages
                    .iter()
                    .filter(|p| selected.contains(&p.key))
                    .cloned()
                    .collect();
                (!pages.is_empty() || selected.contains(&f.key))
                    .then(|| BlueprintFolder { pages, ..f.clone() })
            })
            .collect(),
    }
}

/// `buildFromBlueprint`: wiki folders and template pages, ids drawn once per folder and page
/// in order.
pub fn build_from_blueprint(
    blueprint: &Blueprint,
    new_id: &mut dyn FnMut() -> String,
) -> (Vec<JsValue>, Vec<JsValue>) {
    let mut folders = Vec::new();
    let mut documents = Vec::new();
    for spec in &blueprint.folders {
        let folder_id = new_id();
        let mut folder = JsObject::new();
        folder.insert("id", JsValue::str(&folder_id));
        folder.insert("title", JsValue::str(&spec.name));
        folder.insert("parentId", JsValue::Null);
        folders.push(JsValue::Object(folder));
        for page in &spec.pages {
            let mut doc = JsObject::new();
            doc.insert("id", JsValue::str(&new_id()));
            doc.insert("title", JsValue::str(&page.title));
            doc.insert("content", JsValue::str(&page.html));
            doc.insert("space", JsValue::str("Wiki"));
            doc.insert("folderId", JsValue::str(&folder_id));
            documents.push(JsValue::Object(doc));
        }
    }
    (folders, documents)
}

/// The blueprint ProseCraft starts a screenplay with: Characters, Places and Items.
pub fn screenplay_blueprint() -> Blueprint {
    ["characters", "places", "items"]
        .iter()
        .fold(Blueprint::default(), |b, id| add_module(&b, id))
}

/// `createProjectWithStarter`: a new project's data. `existing` is how many live projects
/// there are (the name is "Untitled Project", then "Untitled Project 2"…). Ids are drawn in
/// ProseCraft's order: project, chapter, storyboard row, first scene, then the wiki.
/// ProseCraft's `defaultPublishOptions()`.
pub fn default_publish_options() -> JsValue {
    data().publish_options.clone()
}

pub fn new_project(
    screenplay: bool,
    existing: usize,
    blueprint: &Blueprint,
    new_id: &mut dyn FnMut() -> String,
) -> JsValue {
    let number = existing + 1;
    let id = new_id();
    let mut chapter = JsObject::new();
    let chapter_id = new_id();
    chapter.insert("id", JsValue::str(&chapter_id));
    chapter.insert(
        "title",
        JsValue::str(if screenplay { "Act 1" } else { "Chapter 1" }),
    );
    chapter.insert("expanded", JsValue::Bool(true));
    chapter.insert("summary", JsValue::str(""));
    let mut row = JsObject::new();
    row.insert("id", JsValue::str(&new_id()));
    row.insert("title", JsValue::str("Timeline"));
    let mut scene = JsObject::new();
    let scene_id = new_id();
    scene.insert("id", JsValue::str(&scene_id));
    scene.insert("title", JsValue::str("Scene 1"));
    scene.insert(
        "content",
        JsValue::str(if screenplay {
            r#"<p data-sp="scene-heading"></p>"#
        } else {
            "<p><br></p>"
        }),
    );
    scene.insert("chapterId", JsValue::str(&chapter_id));
    if screenplay {
        scene.insert("format", JsValue::str("screenplay"));
    }
    let (folders, wiki) = build_from_blueprint(blueprint, new_id);
    let mut documents = vec![JsValue::Object(scene)];
    documents.extend(wiki);
    let mut p = JsObject::new();
    p.insert("id", JsValue::str(&id));
    p.insert(
        "name",
        JsValue::str(&if number == 1 {
            "Untitled Project".to_owned()
        } else {
            format!("Untitled Project {number}")
        }),
    );
    p.insert(
        "kind",
        JsValue::str(if screenplay { "screenplay" } else { "novel" }),
    );
    p.insert("author", JsValue::str(""));
    p.insert("documents", JsValue::Array(documents));
    p.insert("chapters", JsValue::Array(vec![JsValue::Object(chapter)]));
    p.insert("activeDocumentId", JsValue::str(&scene_id));
    p.insert("projectGoal", JsValue::Number(50000.0));
    p.insert("chapterGoals", JsValue::Object(JsObject::new()));
    p.insert("activity", JsValue::Object(JsObject::new()));
    p.insert("storyboardCards", JsValue::Array(Vec::new()));
    p.insert("storyboardRows", JsValue::Array(vec![JsValue::Object(row)]));
    p.insert("timelineNodes", JsValue::Array(Vec::new()));
    p.insert("researchFolderName", JsValue::str(""));
    p.insert("researchFolders", JsValue::Array(folders));
    p.insert("researchItems", JsValue::Array(Vec::new()));
    p.insert("publishOptions", data().publish_options.clone());
    p.insert("citations", JsValue::Array(Vec::new()));
    JsValue::Object(p)
}

#[cfg(test)]
mod tests {
    use super::{Blueprint, new_project, screenplay_blueprint};
    use crate::js::{JsValue, stringify};

    #[test]
    fn a_new_project_is_prosecrafts_new_project() {
        let mut n = 0;
        let mut id = || {
            n += 1;
            format!("u{n}")
        };
        let p = new_project(false, 0, &Blueprint::default(), &mut id);
        let head = stringify(&p);
        assert!(head.starts_with(r#"{"id":"u1","name":"Untitled Project","kind":"novel","author":"","documents":[{"id":"u4","title":"Scene 1","content":"<p><br></p>","chapterId":"u2"}],"chapters":[{"id":"u2","title":"Chapter 1","expanded":true,"summary":""}],"activeDocumentId":"u4","projectGoal":50000,"chapterGoals":{},"activity":{},"storyboardCards":[],"storyboardRows":[{"id":"u3","title":"Timeline"}],"timelineNodes":[],"researchFolderName":"","researchFolders":[],"researchItems":[],"publishOptions":{"format":"pdf""#), "{head}");
        assert!(head.ends_with(r#""citations":[]}"#));
        let screenplay = new_project(true, 2, &screenplay_blueprint(), &mut || "x".to_owned());
        assert_eq!(
            screenplay
                .get("name")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .as_deref(),
            Some("Untitled Project 3")
        );
        let docs = screenplay
            .get("documents")
            .and_then(JsValue::as_array)
            .expect("docs");
        assert_eq!(
            docs.len(),
            1 + 4,
            "the scene and the Characters, Places and Items pages"
        );
        assert!(stringify(&docs[0]).contains(
            r#""content":"<p data-sp=\"scene-heading\"></p>","chapterId":"x","format":"screenplay""#
        ));
    }
}
