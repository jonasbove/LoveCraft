//! The Wiki's pages and folders (ProseCraft's `createSpaceDocument`, `createResearchFolder`,
//! `saveResearchFolderRename`, `removeResearchFolder` and `moveResearchDocument`, upstream
//! `7dae544`), on the stored project.

use crate::edit::EditError;
use crate::html::is_js_whitespace;
use crate::js::{JsObject, JsValue};

/// A new page's title (`WIKI_PAGE_TITLE`).
pub const PAGE_TITLE: &str = "Untitled page";
pub const FOLDER_TITLE: &str = "Untitled folder";
const FOLDER_TITLE_LIMIT: usize = 60;

fn list_mut<'a>(project: &'a mut JsValue, key: &str) -> Result<&'a mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(object.get(key), Some(JsValue::Array(_))) {
        object.insert(key, JsValue::Array(Vec::new()));
    }
    match object.get_mut(key) {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn id_is(v: &JsValue, key: &str, id: &str) -> bool {
    v.get(key)
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

fn folder_exists(project: &JsValue, id: &str) -> bool {
    project
        .get("researchFolders")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .any(|f| id_is(f, "id", id))
}

/// `createSpaceDocument("Wiki", folderId)`: an empty page, last.
pub fn add_page(project: &mut JsValue, id: &str, folder: Option<&str>) -> Result<(), EditError> {
    let mut doc = JsObject::new();
    doc.insert("id", JsValue::str(id));
    doc.insert("title", JsValue::str(PAGE_TITLE));
    doc.insert("content", JsValue::str("<p><br></p>"));
    doc.insert("space", JsValue::str("Wiki"));
    if let Some(folder) = folder {
        doc.insert("folderId", JsValue::str(folder));
    }
    list_mut(project, "documents")?.push(JsValue::Object(doc));
    Ok(())
}

/// `createResearchFolder(parentId)`.
pub fn add_folder(project: &mut JsValue, id: &str, parent: Option<&str>) -> Result<(), EditError> {
    let mut folder = JsObject::new();
    folder.insert("id", JsValue::str(id));
    folder.insert("title", JsValue::str(FOLDER_TITLE));
    folder.insert("parentId", parent.map_or(JsValue::Null, JsValue::str));
    list_mut(project, "researchFolders")?.push(JsValue::Object(folder));
    Ok(())
}

/// `saveResearchFolderRename`: trimmed, at most 60 characters; a blank name changes nothing.
pub fn rename_folder(project: &mut JsValue, id: &str, title: &str) -> Result<(), EditError> {
    let units: Vec<u16> = title
        .trim_matches(is_js_whitespace)
        .encode_utf16()
        .take(FOLDER_TITLE_LIMIT)
        .collect();
    if units.is_empty() {
        return Ok(());
    }
    let folder = list_mut(project, "researchFolders")?
        .iter_mut()
        .find(|f| id_is(f, "id", id))
        .ok_or(EditError::Shape)?;
    folder.as_object_mut().ok_or(EditError::Shape)?.insert(
        "title",
        JsValue::String(crate::js::JsString::from_units(units)),
    );
    Ok(())
}

/// `removeResearchFolder`: the folder goes; its subfolders, pages, files and links move to
/// its parent (or the Wiki root). Nothing else is deleted.
pub fn remove_folder(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    let folders = list_mut(project, "researchFolders")?;
    let Some(folder) = folders.iter().find(|f| id_is(f, "id", id)).cloned() else {
        return Ok(());
    };
    let parent = folder
        .get("parentId")
        .and_then(JsValue::as_js_string)
        .map(|p| p.to_string_lossy());
    folders.retain(|f| !id_is(f, "id", id));
    for f in folders.iter_mut().filter(|f| id_is(f, "parentId", id)) {
        if let Some(o) = f.as_object_mut() {
            o.insert(
                "parentId",
                parent.as_deref().map_or(JsValue::Null, JsValue::str),
            );
        }
    }
    for doc in list_mut(project, "documents")?
        .iter_mut()
        .filter(|d| id_is(d, "space", "Wiki") && id_is(d, "folderId", id))
    {
        if let Some(o) = doc.as_object_mut() {
            match &parent {
                Some(p) => o.insert("folderId", JsValue::str(p)),
                None => {
                    o.remove("folderId");
                }
            }
        }
    }
    for item in list_mut(project, "researchItems")?
        .iter_mut()
        .filter(|i| id_is(i, "folderId", id))
    {
        if let Some(o) = item.as_object_mut() {
            o.insert(
                "folderId",
                parent.as_deref().map_or(JsValue::Null, JsValue::str),
            );
        }
    }
    Ok(())
}

/// `moveResearchDocument`: a Wiki page into a folder, or unfiled (`None`).
pub fn move_page(
    project: &mut JsValue,
    document: &str,
    folder: Option<&str>,
) -> Result<(), EditError> {
    if folder.is_some_and(|f| !folder_exists(project, f)) {
        return Ok(());
    }
    for doc in list_mut(project, "documents")?
        .iter_mut()
        .filter(|d| id_is(d, "id", document) && id_is(d, "space", "Wiki"))
    {
        if let Some(o) = doc.as_object_mut() {
            match folder {
                Some(f) => o.insert("folderId", JsValue::str(f)),
                None => {
                    o.remove("folderId");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{add_folder, add_page, move_page, remove_folder, rename_folder};
    use crate::js::{parse, stringify};

    #[test]
    fn pages_and_folders_as_prosecraft_keeps_them() {
        let mut p = parse(r#"{"documents":[],"researchFolders":[{"id":"top","title":"Top","parentId":null}],"researchItems":[]}"#).expect("json");
        add_folder(&mut p, "f", Some("top")).expect("folder");
        rename_folder(&mut p, "f", "  People  ").expect("rename");
        rename_folder(&mut p, "f", "   ").expect("blank changes nothing");
        add_folder(&mut p, "g", Some("f")).expect("subfolder");
        add_page(&mut p, "a", Some("f")).expect("page");
        add_page(&mut p, "b", None).expect("page");
        move_page(&mut p, "b", Some("g")).expect("move");
        move_page(&mut p, "b", Some("missing")).expect("an unknown folder changes nothing");
        assert_eq!(
            stringify(&p),
            r#"{"documents":[{"id":"a","title":"Untitled page","content":"<p><br></p>","space":"Wiki","folderId":"f"},{"id":"b","title":"Untitled page","content":"<p><br></p>","space":"Wiki","folderId":"g"}],"researchFolders":[{"id":"top","title":"Top","parentId":null},{"id":"f","title":"People","parentId":"top"},{"id":"g","title":"Untitled folder","parentId":"f"}],"researchItems":[]}"#
        );
        remove_folder(&mut p, "f").expect("remove");
        assert_eq!(
            stringify(&p),
            r#"{"documents":[{"id":"a","title":"Untitled page","content":"<p><br></p>","space":"Wiki","folderId":"top"},{"id":"b","title":"Untitled page","content":"<p><br></p>","space":"Wiki","folderId":"g"}],"researchFolders":[{"id":"top","title":"Top","parentId":null},{"id":"g","title":"Untitled folder","parentId":"top"}],"researchItems":[]}"#
        );
        remove_folder(&mut p, "top").expect("remove to the root");
        assert!(stringify(&p).contains(
            r#"{"id":"a","title":"Untitled page","content":"<p><br></p>","space":"Wiki"}"#
        ));
    }
}
