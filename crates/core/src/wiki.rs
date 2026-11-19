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

/// What is dragged or dropped on in the Wiki (`WikiDragKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder,
    Document,
    File,
    Link,
}

impl Kind {
    /// The stored list a kind lives in, and its parent field.
    fn list(self) -> (&'static str, &'static str) {
        match self {
            Kind::Folder => ("researchFolders", "parentId"),
            Kind::Document => ("documents", "folderId"),
            Kind::File | Kind::Link => ("researchItems", "folderId"),
        }
    }
}

fn text_of(v: &JsValue, key: &str) -> Option<String> {
    v.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

/// `wikiParentOf`: the folder a row is in (`Some(None)` for the top level); `None` when there is
/// no such row.
fn parent_of(project: &JsValue, kind: Kind, id: &str) -> Option<Option<String>> {
    let (list, field) = kind.list();
    let row = project
        .get(list)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|r| id_is(r, "id", id))?;
    if kind == Kind::Document && !id_is(row, "space", "Wiki") {
        return None;
    }
    Some(text_of(row, field))
}

/// `isFolderDescendant`: whether `folder` is inside `ancestor`.
fn is_descendant(project: &JsValue, folder: &str, ancestor: &str) -> bool {
    let folders = project
        .get("researchFolders")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    let parent = |id: &str| {
        folders
            .iter()
            .find(|f| id_is(f, "id", id))
            .and_then(|f| text_of(f, "parentId"))
    };
    let mut current = parent(folder);
    let mut steps = 0;
    while let Some(id) = current {
        if id == ancestor || steps > folders.len() {
            return id == ancestor;
        }
        current = parent(&id);
        steps += 1;
    }
    false
}

/// Sets a row's parent: a folder, or none (a folder's `parentId: null`; a page's or item's field
/// as ProseCraft writes it).
fn set_parent(row: &mut JsValue, kind: Kind, folder: Option<&str>) {
    let (_, field) = kind.list();
    if let Some(o) = row.as_object_mut() {
        match (folder, kind) {
            (Some(f), _) => o.insert(field, JsValue::str(f)),
            (None, Kind::Document) => {
                o.remove(field);
            }
            (None, _) => o.insert(field, JsValue::Null),
        }
    }
}

/// `arrayMoveTo`: `id` taken out and put before or after `target`.
fn move_to(list: &mut Vec<JsValue>, id: &str, target: &str, after: bool) {
    let Some(from) = list.iter().position(|r| id_is(r, "id", id)) else {
        return;
    };
    let row = list.remove(from);
    match list.iter().position(|r| id_is(r, "id", target)) {
        Some(at) => list.insert(at + usize::from(after), row),
        None => list.insert(from, row),
    }
}

/// `moveWikiFolder`: a folder a place up or down among its siblings.
pub fn move_folder(project: &mut JsValue, id: &str, offset: isize) -> Result<(), EditError> {
    let folders = list_mut(project, "researchFolders")?;
    let Some(parent) = folders
        .iter()
        .find(|f| id_is(f, "id", id))
        .map(|f| text_of(f, "parentId"))
    else {
        return Ok(());
    };
    let siblings: Vec<usize> = (0..folders.len())
        .filter(|i| text_of(&folders[*i], "parentId") == parent)
        .collect();
    let Some(index) = siblings.iter().position(|i| id_is(&folders[*i], "id", id)) else {
        return Ok(());
    };
    if let Some(target) = index
        .checked_add_signed(offset)
        .filter(|t| *t < siblings.len())
    {
        folders.swap(siblings[index], siblings[target]);
    }
    Ok(())
}

/// `moveWikiFolderToTop`: a folder at the top level, last.
pub fn move_folder_to_top(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    move_into_folder(project, Kind::Folder, id, None)
}

/// `moveWikiIntoFolder`: a row dropped into a folder (or the top level), last in that group. A
/// folder isn't moved into itself or its own subfolders.
pub fn move_into_folder(
    project: &mut JsValue,
    kind: Kind,
    id: &str,
    folder: Option<&str>,
) -> Result<(), EditError> {
    if folder.is_some_and(|f| !folder_exists(project, f)) {
        return Ok(());
    }
    let Some(parent) = parent_of(project, kind, id) else {
        return Ok(());
    };
    if kind == Kind::Folder {
        if parent.as_deref() == folder {
            return Ok(());
        }
        if folder.is_some_and(|f| f == id || is_descendant(project, f, id)) {
            return Ok(());
        }
    }
    let (list, _) = kind.list();
    let rows = list_mut(project, list)?;
    let Some(from) = rows.iter().position(|r| id_is(r, "id", id)) else {
        return Ok(());
    };
    let mut row = rows.remove(from);
    set_parent(&mut row, kind, folder);
    rows.push(row);
    Ok(())
}

/// `moveWikiToRow`: a row dropped before or after another. Folders order among folders (taking
/// the target's parent); a folder dropped on another kind of row goes last in that row's group;
/// anything else dropped on a folder goes into the folder's parent; pages and items order within
/// their own lists, taking the target's folder.
pub fn move_to_row(
    project: &mut JsValue,
    (kind, id): (Kind, &str),
    (target_kind, target): (Kind, &str),
    after: bool,
) -> Result<(), EditError> {
    if id == target {
        return Ok(());
    }
    match (kind, target_kind) {
        (Kind::Folder, Kind::Folder) => {
            let Some(parent) = parent_of(project, Kind::Folder, target) else {
                return Ok(());
            };
            if parent_of(project, Kind::Folder, id).is_none() {
                return Ok(());
            }
            let folders = list_mut(project, "researchFolders")?;
            if let Some(row) = folders.iter_mut().find(|f| id_is(f, "id", id)) {
                set_parent(row, Kind::Folder, parent.as_deref());
            }
            move_to(folders, id, target, after);
            Ok(())
        }
        (Kind::Folder, _) => match parent_of(project, target_kind, target) {
            Some(parent) => move_into_folder(project, kind, id, parent.as_deref()),
            None => Ok(()),
        },
        (_, Kind::Folder) => match parent_of(project, Kind::Folder, target) {
            Some(parent) => move_into_folder(project, kind, id, parent.as_deref()),
            None => Ok(()),
        },
        _ => {
            let Some(parent) = parent_of(project, target_kind, target) else {
                return Ok(());
            };
            // The row must be there (a page in the Wiki); dropped on a row of another list it
            // only takes that row's folder, as `arrayMoveTo` finds no target there.
            if parent_of(project, kind, id).is_none() {
                return Ok(());
            }
            let (list, _) = kind.list();
            let rows = list_mut(project, list)?;
            if let Some(row) = rows.iter_mut().find(|r| id_is(r, "id", id)) {
                set_parent(row, kind, parent.as_deref());
            }
            move_to(rows, id, target, after);
            Ok(())
        }
    }
}

#[cfg(test)]
mod move_tests {
    use super::{Kind, move_folder, move_folder_to_top, move_into_folder, move_to_row};
    use crate::js::{JsValue, parse};

    fn project() -> JsValue {
        parse(
            r#"{"researchFolders":[{"id":"a","title":"A","parentId":null},{"id":"b","title":"B","parentId":null},{"id":"c","title":"C","parentId":"a"},{"id":"d","title":"D","parentId":"a"}],
            "documents":[{"id":"p1","space":"Wiki"},{"id":"p2","space":"Wiki","folderId":"a"},{"id":"s","chapterId":"ch"}],
            "researchItems":[{"id":"f1","kind":"file","folderId":null},{"id":"l1","kind":"link","folderId":"a"}]}"#,
        )
        .expect("json")
    }

    fn rows(p: &JsValue, list: &str, field: &str) -> String {
        p.get(list)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|r| {
                let id = r
                    .get("id")
                    .and_then(JsValue::as_js_string)
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default();
                let parent = r
                    .get(field)
                    .and_then(JsValue::as_js_string)
                    .map(|s| s.to_string_lossy())
                    .unwrap_or("-".into());
                format!("{id}@{parent}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn folders_move_among_siblings_and_to_the_top() {
        let mut p = project();
        move_folder(&mut p, "d", -1).expect("up");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- b@- d@a c@a");
        move_folder(&mut p, "a", -1).expect("first stays");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- b@- d@a c@a");
        move_folder_to_top(&mut p, "c").expect("top");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- b@- d@a c@-");
    }

    #[test]
    fn rows_drop_into_folders_and_beside_rows() {
        let mut p = project();
        // Not into itself or its own subfolder.
        move_into_folder(&mut p, Kind::Folder, "a", Some("c")).expect("into child");
        move_into_folder(&mut p, Kind::Folder, "a", Some("a")).expect("into self");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- b@- c@a d@a");
        move_into_folder(&mut p, Kind::Folder, "b", Some("c")).expect("into c");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- c@a d@a b@c");
        // A page into a folder, last; then to the top level.
        move_into_folder(&mut p, Kind::Document, "p1", Some("b")).expect("page");
        assert_eq!(rows(&p, "documents", "folderId"), "p2@a s@- p1@b");
        move_into_folder(&mut p, Kind::Document, "p1", None).expect("page out");
        assert_eq!(rows(&p, "documents", "folderId"), "p2@a s@- p1@-");
        // A page before another, taking its folder.
        move_to_row(
            &mut p,
            (Kind::Document, "p1"),
            (Kind::Document, "p2"),
            false,
        )
        .expect("row");
        assert_eq!(rows(&p, "documents", "folderId"), "p1@a p2@a s@-");
        // A folder beside a folder takes its parent and place.
        move_to_row(&mut p, (Kind::Folder, "b"), (Kind::Folder, "a"), true).expect("folder row");
        assert_eq!(rows(&p, "researchFolders", "parentId"), "a@- b@- c@a d@a");
        // A file dropped on a folder goes into that folder's parent; on a link, beside it.
        move_to_row(&mut p, (Kind::File, "f1"), (Kind::Folder, "c"), false).expect("on folder");
        assert_eq!(rows(&p, "researchItems", "folderId"), "l1@a f1@a");
        move_to_row(&mut p, (Kind::File, "f1"), (Kind::Link, "l1"), false).expect("on a link");
        assert_eq!(rows(&p, "researchItems", "folderId"), "f1@a l1@a");
        // On a page: the page's folder, its place in its own list kept.
        move_to_row(&mut p, (Kind::File, "f1"), (Kind::Document, "s"), false).expect("on a scene");
        move_to_row(&mut p, (Kind::Link, "l1"), (Kind::Document, "p1"), true).expect("on a page");
        assert_eq!(rows(&p, "researchItems", "folderId"), "f1@a l1@a");
    }
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
