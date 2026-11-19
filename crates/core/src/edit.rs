//! Edits to a project, as ProseCraft makes them (`src/routes/+page.svelte`, upstream `7dae544`).
//!
//! Each operation changes the project's data (`StoredProject.project`) in place and keeps every
//! field it doesn't own; the caller then stamps it (`storage::stamp::stamp_project`) and saves.
//! New records have exactly ProseCraft's shape, so ProseCraft reads them as its own.

use crate::html::{Node, parse_fragment};
use crate::js::{JsObject, JsValue};

/// Why an edit wasn't made.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditError {
    #[error("the project data is not in the expected shape")]
    Shape,
    #[error("there is no chapter {0}")]
    NoChapter(String),
    #[error("there is no document {0}")]
    NoDocument(String),
}

fn list_mut<'a>(project: &'a mut JsValue, key: &str) -> Result<&'a mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if object.get(key).is_none() {
        object.insert(key, JsValue::Array(Vec::new()));
    }
    match object.get_mut(key) {
        Some(JsValue::Array(items)) => Ok(items),
        _ => Err(EditError::Shape),
    }
}

fn has_id(record: &JsValue, id: &str) -> bool {
    record
        .get("id")
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

fn find_mut<'a>(
    project: &'a mut JsValue,
    key: &str,
    id: &str,
) -> Result<&'a mut JsObject, EditError> {
    let missing = || {
        if key == "chapters" {
            EditError::NoChapter(id.to_owned())
        } else {
            EditError::NoDocument(id.to_owned())
        }
    };
    list_mut(project, key)?
        .iter_mut()
        .find(|r| has_id(r, id))
        .and_then(JsValue::as_object_mut)
        .ok_or_else(missing)
}

fn text(value: Option<&JsValue>) -> String {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

fn flag(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Bool(true)))
}

/// `isAutomaticallyNumberedChapter`: a title like "Chapter 12" (ASCII digits, as JavaScript's `\d`).
pub fn is_auto_numbered(title: &str) -> bool {
    title
        .strip_prefix("Chapter ")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// `chapterLabel`: an automatically numbered chapter shows its place among them, so numbers stay in
/// order after a chapter moves or goes to the trash; any other chapter shows its title.
pub fn chapter_label(title: &str, live_titles_before_and_including: &[&str]) -> String {
    if !is_auto_numbered(title) {
        return title.to_owned();
    }
    let number = live_titles_before_and_including
        .iter()
        .filter(|t| is_auto_numbered(t))
        .count();
    format!("Chapter {number}")
}

/// `createScene` with its defaults: an empty scene, a screenplay one in a screenplay project.
pub fn add_scene(
    project: &mut JsValue,
    chapter_id: &str,
    title: &str,
    id: String,
) -> Result<String, EditError> {
    add_scene_with(project, chapter_id, title, id, None, None)
}

/// `createScene(chapterId, title, { content, format })`: the content given or the empty scene
/// of its format; the format given, or "screenplay" in a screenplay project.
pub fn add_scene_with(
    project: &mut JsValue,
    chapter_id: &str,
    title: &str,
    id: String,
    content: Option<&str>,
    format: Option<&str>,
) -> Result<String, EditError> {
    let screenplay_project = project
        .get("kind")
        .and_then(JsValue::as_js_string)
        .is_some_and(|k| k.eq_str("screenplay"));
    let format = format.or(screenplay_project.then_some("screenplay"));
    let empty = if format == Some("screenplay") {
        r#"<p data-sp="scene-heading"></p>"#
    } else {
        "<p><br></p>"
    };
    let mut scene = JsObject::new();
    scene.insert("id", JsValue::str(&id));
    scene.insert("title", JsValue::str(title));
    scene.insert("content", JsValue::str(content.unwrap_or(empty)));
    scene.insert("chapterId", JsValue::str(chapter_id));
    if let Some(format) = format {
        scene.insert("format", JsValue::str(format));
    }
    list_mut(project, "documents")?.push(JsValue::Object(scene));
    Ok(id)
}

/// `createChapter`: "Chapter N" (N counts the live automatically numbered chapters) with one
/// scene, "Scene 1".
pub fn add_chapter(
    project: &mut JsValue,
    chapter_id: String,
    scene_id: String,
) -> Result<(String, String), EditError> {
    let chapters = list_mut(project, "chapters")?;
    let number = chapters
        .iter()
        .filter(|c| !flag(c.get("trashed")) && is_auto_numbered(&text(c.get("title"))))
        .count()
        + 1;
    let mut chapter = JsObject::new();
    chapter.insert("id", JsValue::str(&chapter_id));
    chapter.insert("title", JsValue::str(&format!("Chapter {number}")));
    chapter.insert("expanded", JsValue::Bool(true));
    chapter.insert("summary", JsValue::str(""));
    chapters.push(JsValue::Object(chapter));
    let scene = add_scene(project, &chapter_id, "Scene 1", scene_id)?;
    Ok((chapter_id, scene))
}

pub fn set_chapter_title(project: &mut JsValue, id: &str, title: &str) -> Result<(), EditError> {
    find_mut(project, "chapters", id)?.insert("title", JsValue::str(title));
    Ok(())
}

pub fn set_chapter_summary(
    project: &mut JsValue,
    id: &str,
    summary: &str,
) -> Result<(), EditError> {
    // `syncChapterSummary`: at most 4000, and the chapter's storyboard cards follow.
    find_mut(project, "chapters", id)?;
    crate::storyboard::sync_chapter_summary(project, id, summary)
}

/// Sets one of the project's `publishOptions`, keeping the others (and any this version
/// doesn't know). A missing or malformed `publishOptions` starts as an empty object, which
/// ProseCraft reads with its defaults.
pub fn set_publish_option(
    project: &mut JsValue,
    key: &str,
    value: JsValue,
) -> Result<(), EditError> {
    let data = project.as_object_mut().ok_or(EditError::Shape)?;
    if data
        .get("publishOptions")
        .and_then(JsValue::as_object)
        .is_none()
    {
        data.insert(
            "publishOptions",
            JsValue::Object(crate::js::JsObject::new()),
        );
    }
    data.get_mut("publishOptions")
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?
        .insert(key, value);
    Ok(())
}

pub fn set_document_title(project: &mut JsValue, id: &str, title: &str) -> Result<(), EditError> {
    find_mut(project, "documents", id)?.insert("title", JsValue::str(title));
    Ok(())
}

/// New rich content for a document. ProseCraft's rule: a rich edit removes `markdownSource`,
/// which would otherwise win over the new content.
pub fn set_document_content(project: &mut JsValue, id: &str, html: &str) -> Result<(), EditError> {
    let document = find_mut(project, "documents", id)?;
    document.insert("content", JsValue::str(html));
    document.remove("markdownSource");
    Ok(())
}

/// `updateTileDocument` with `patchTileDocument`: an edit made in a split view pane. A
/// scene's pane edits its whole chapter, so its text goes back to the chapter's scenes
/// (`splitChapterHtml`, `writeChapterSectionsFor`); any other document takes the title and the
/// text, a changed text dropping its Markdown source. An unknown document is left alone.
pub fn patch_tile_document(
    project: &mut JsValue,
    id: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> Result<(), EditError> {
    let text_of = |v: Option<&JsValue>| {
        v.and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    let docs = project
        .get("documents")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let Some(doc) = docs
        .iter()
        .find(|d| text_of(d.get("id")).as_deref() == Some(id))
    else {
        return Ok(());
    };
    let chapter = text_of(doc.get("chapterId"));
    let chapter_exists = chapter.as_ref().is_some_and(|c| {
        project
            .get("chapters")
            .and_then(JsValue::as_array)
            .is_some_and(|list| {
                list.iter()
                    .any(|ch| text_of(ch.get("id")).as_ref() == Some(c))
            })
    });
    if let (Some(content), Some(chapter), true) = (content, chapter.as_deref(), chapter_exists) {
        let sections = crate::chapter::split_chapter_html(&crate::html::sanitize_html(content));
        return crate::chapter::write_chapter_sections(project, chapter, &sections, None, None);
    }
    patch_document(project, id, title, content)
}

/// `patchTileDocument` alone: the title and the text of one document (a scene edited on its
/// own in a pane), a changed text dropping its Markdown source.
pub fn patch_document(
    project: &mut JsValue,
    id: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> Result<(), EditError> {
    let text_of = |v: Option<&JsValue>| {
        v.and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    let document = find_mut(project, "documents", id)?;
    if let Some(title) = title {
        document.insert("title", JsValue::str(title));
    }
    let current = text_of(document.get("content"));
    if let Some(content) = content
        && current.as_deref() != Some(content)
    {
        document.insert("content", JsValue::str(content));
        document.remove("markdownSource");
    }
    Ok(())
}

/// `applySingleDocumentMarkdown`: Markdown from the Markdown editor as the document's content
/// (sanitized), kept as its `markdownSource`, and its footnote definitions as the document's
/// footnotes (replacing the old ones; none when it has none).
pub fn set_document_markdown(
    project: &mut JsValue,
    id: &str,
    source: &str,
) -> Result<(), EditError> {
    let parsed = crate::markdown::markdown_to_document(source);
    let html = crate::html::sanitize_html(&parsed.html);
    let document = find_mut(project, "documents", id)?;
    document.remove("footnotes");
    document.insert("content", JsValue::str(&html));
    document.insert("markdownSource", JsValue::str(source));
    if !parsed.footnotes.is_empty() {
        let notes = parsed
            .footnotes
            .iter()
            .map(|f| {
                let mut note = crate::js::JsObject::new();
                note.insert("id", JsValue::str(&f.id));
                note.insert("html", JsValue::str(&f.html));
                JsValue::Object(note)
            })
            .collect();
        document.insert("footnotes", JsValue::Array(notes));
    }
    Ok(())
}

/// The project's citation library (`citations`), as the Sources dialog leaves it.
pub fn set_citations(project: &mut JsValue, items: Vec<JsValue>) -> Result<(), EditError> {
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("citations", JsValue::Array(items));
    Ok(())
}

pub fn set_project_name(project: &mut JsValue, name: &str) -> Result<(), EditError> {
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("name", JsValue::str(name));
    Ok(())
}

/// `saveProjectSettings`: the name (trimmed, required), the author (trimmed) and the type.
/// ProseCraft limits both fields to 100 characters in the form.
pub fn set_project_details(
    project: &mut JsValue,
    name: &str,
    author: &str,
    screenplay: bool,
) -> Result<(), EditError> {
    let name = name.trim_matches(crate::html::is_js_whitespace);
    if name.is_empty() {
        return Err(EditError::Shape);
    }
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    object.insert("name", JsValue::str(name));
    object.insert(
        "author",
        JsValue::str(author.trim_matches(crate::html::is_js_whitespace)),
    );
    object.insert(
        "kind",
        JsValue::str(if screenplay { "screenplay" } else { "novel" }),
    );
    Ok(())
}

/// A goal as ProseCraft keeps it: a whole number, never negative (`Math.max(0, Math.floor(n))`,
/// 0 when not a number).
fn goal(value: f64) -> f64 {
    if value.is_finite() {
        value.floor().max(0.0)
    } else {
        0.0
    }
}

/// `setProjectGoal`.
pub fn set_project_goal(project: &mut JsValue, value: f64) -> Result<(), EditError> {
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("projectGoal", JsValue::Number(goal(value)));
    Ok(())
}

/// `setChapterGoal`: the chapter's entry in `chapterGoals`.
pub fn set_chapter_goal(
    project: &mut JsValue,
    chapter_id: &str,
    value: f64,
) -> Result<(), EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if object
        .get("chapterGoals")
        .and_then(JsValue::as_object)
        .is_none()
    {
        object.insert("chapterGoals", JsValue::Object(crate::js::JsObject::new()));
    }
    object
        .get_mut("chapterGoals")
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?
        .insert(chapter_id, JsValue::Number(goal(value)));
    Ok(())
}

/// `trackActivity`, for several edits at once: the day's `{ edits, wordsAdded }` grow by
/// `edits` and by the words added (never less than 0). `day` is `YYYY-MM-DD`.
pub fn track_activity(
    project: &mut JsValue,
    day: &str,
    edits: f64,
    words_added: f64,
) -> Result<(), EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if object
        .get("activity")
        .and_then(JsValue::as_object)
        .is_none()
    {
        object.insert("activity", JsValue::Object(crate::js::JsObject::new()));
    }
    let activity = object
        .get_mut("activity")
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?;
    let previous = |k: &str| {
        activity
            .get(day)
            .and_then(|d| d.get(k))
            .and_then(JsValue::as_number)
            .unwrap_or(0.0)
    };
    let mut entry = crate::js::JsObject::new();
    entry.insert("edits", JsValue::Number(previous("edits") + edits));
    entry.insert(
        "wordsAdded",
        JsValue::Number(previous("wordsAdded") + words_added.max(0.0)),
    );
    activity.insert(day, JsValue::Object(entry));
    Ok(())
}

/// `moveToTrash` for one document.
pub fn trash_document(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    find_mut(project, "documents", id)?.insert("trashed", JsValue::Bool(true));
    Ok(())
}

/// `trashChapter`: the chapter and its live scenes, which remember they went with it.
pub fn trash_chapter(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    find_mut(project, "chapters", id)?.insert("trashed", JsValue::Bool(true));
    for document in list_mut(project, "documents")?.iter_mut() {
        let in_chapter = document
            .get("chapterId")
            .and_then(JsValue::as_js_string)
            .is_some_and(|c| c.eq_str(id));
        let live = !flag(document.get("trashed"))
            && document
                .get("space")
                .is_none_or(|s| text(Some(s)).is_empty());
        if in_chapter
            && live
            && let Some(object) = document.as_object_mut()
        {
            object.insert("trashed", JsValue::Bool(true));
            object.insert("trashedWithChapterId", JsValue::str(id));
        }
    }
    Ok(())
}

/// `restoreChapter`: the chapter and the scenes that went to the trash with it (and
/// `restore_document`, when given, even if it was trashed on its own).
pub fn restore_chapter(
    project: &mut JsValue,
    id: &str,
    restore_document: Option<&str>,
) -> Result<(), EditError> {
    find_mut(project, "chapters", id)?.insert("trashed", JsValue::Bool(false));
    for document in list_mut(project, "documents")?.iter_mut() {
        let in_chapter = document
            .get("chapterId")
            .and_then(JsValue::as_js_string)
            .is_some_and(|c| c.eq_str(id));
        let with_chapter = document
            .get("trashedWithChapterId")
            .and_then(JsValue::as_js_string)
            .is_some_and(|c| c.eq_str(id));
        let asked = restore_document.is_some_and(|d| has_id(document, d));
        if in_chapter
            && (with_chapter || asked)
            && let Some(object) = document.as_object_mut()
        {
            object.remove("trashedWithChapterId");
            object.insert("trashed", JsValue::Bool(false));
        }
    }
    Ok(())
}

/// `restoreDocument`: a scene whose chapter is in the trash brings the chapter back with it.
pub fn restore_document(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    let chapter_id = list_mut(project, "documents")?
        .iter()
        .find(|d| has_id(d, id))
        .map(|d| text(d.get("chapterId")));
    let trashed_chapter = chapter_id.filter(|c| !c.is_empty()).filter(|c| {
        project
            .get("chapters")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .any(|ch| has_id(ch, c) && flag(ch.get("trashed")))
    });
    match trashed_chapter {
        Some(chapter) => restore_chapter(project, &chapter, Some(id)),
        None => {
            find_mut(project, "documents", id)?.insert("trashed", JsValue::Bool(false));
            Ok(())
        }
    }
}

fn is_trashed(item: &JsValue) -> bool {
    flag(item.get("trashed"))
}

fn chapter_of(document: &JsValue) -> String {
    text(document.get("chapterId"))
}

/// `moveChapter`: a chapter a place up (`-1`) or down (`1`) among the live chapters; chapters
/// in the trash keep their places.
pub fn move_chapter(project: &mut JsValue, id: &str, offset: isize) -> Result<(), EditError> {
    let chapters = list_mut(project, "chapters")?;
    let live: Vec<usize> = (0..chapters.len())
        .filter(|i| !is_trashed(&chapters[*i]))
        .collect();
    let Some(index) = live.iter().position(|i| has_id(&chapters[*i], id)) else {
        return Ok(());
    };
    let Some(target) = index.checked_add_signed(offset).filter(|t| *t < live.len()) else {
        return Ok(());
    };
    chapters.swap(live[index], live[target]);
    Ok(())
}

/// `reorderChapter` (a chapter dragged in the binder): `id` before or after `target` among the
/// live chapters; chapters in the trash keep their places.
pub fn reorder_chapter(
    project: &mut JsValue,
    id: &str,
    target: &str,
    after: bool,
) -> Result<(), EditError> {
    if id == target {
        return Ok(());
    }
    let chapters = list_mut(project, "chapters")?;
    let live: Vec<usize> = (0..chapters.len())
        .filter(|i| !is_trashed(&chapters[*i]))
        .collect();
    let mut reordered: Vec<JsValue> = live.iter().map(|i| chapters[*i].clone()).collect();
    let Some(from) = reordered.iter().position(|c| has_id(c, id)) else {
        return Ok(());
    };
    if !reordered.iter().any(|c| has_id(c, target)) {
        return Ok(());
    }
    let source = reordered.remove(from);
    let Some(at) = reordered.iter().position(|c| has_id(c, target)) else {
        return Ok(());
    };
    reordered.insert(at + usize::from(after), source);
    for (slot, chapter) in live.into_iter().zip(reordered) {
        chapters[slot] = chapter;
    }
    Ok(())
}

/// `moveSceneToPosition` (a scene dragged onto another): before or after `target`, in its
/// chapter.
pub fn move_scene_to_position(
    project: &mut JsValue,
    id: &str,
    target: &str,
    after: bool,
) -> Result<(), EditError> {
    if id == target {
        return Ok(());
    }
    let documents = list_mut(project, "documents")?;
    let Some(from) = documents.iter().position(|d| has_id(d, id)) else {
        return Ok(());
    };
    let Some(chapter) = documents
        .iter()
        .find(|d| has_id(d, target))
        .map(chapter_of)
        .filter(|c| !c.is_empty())
    else {
        return Ok(());
    };
    let mut scene = documents.remove(from);
    let Some(at) = documents.iter().position(|d| has_id(d, target)) else {
        documents.insert(from, scene);
        return Ok(());
    };
    if chapter_of(&scene) != chapter
        && let Some(object) = scene.as_object_mut()
    {
        object.insert("chapterId", JsValue::str(&chapter));
    }
    documents.insert(at + usize::from(after), scene);
    Ok(())
}

/// `moveScene`: a scene a place up or down among its chapter's live scenes.
pub fn move_scene(project: &mut JsValue, id: &str, offset: isize) -> Result<(), EditError> {
    let documents = list_mut(project, "documents")?;
    let Some(chapter) = documents
        .iter()
        .find(|d| has_id(d, id))
        .map(chapter_of)
        .filter(|c| !c.is_empty())
    else {
        return Ok(());
    };
    let list: Vec<usize> = (0..documents.len())
        .filter(|i| chapter_of(&documents[*i]) == chapter && !is_trashed(&documents[*i]))
        .collect();
    let Some(index) = list.iter().position(|i| has_id(&documents[*i], id)) else {
        return Ok(());
    };
    let Some(target) = index.checked_add_signed(offset).filter(|t| *t < list.len()) else {
        return Ok(());
    };
    documents.swap(list[index], list[target]);
    Ok(())
}

/// `moveSceneToChapterEnd` ("Move to chapter…"): the scene after the last scene of `chapter`.
pub fn move_scene_to_chapter_end(
    project: &mut JsValue,
    id: &str,
    chapter: &str,
) -> Result<(), EditError> {
    let live_chapter = project
        .get("chapters")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .any(|c| has_id(c, chapter) && !is_trashed(c));
    if !live_chapter {
        return Ok(());
    }
    let documents = list_mut(project, "documents")?;
    let Some(from) = documents.iter().position(|d| has_id(d, id)) else {
        return Ok(());
    };
    if chapter_of(&documents[from]) == chapter {
        // `moveSceneToPosition` after the chapter's last live scene.
        let last = documents
            .iter()
            .rposition(|d| chapter_of(d) == chapter && !is_trashed(d));
        if let Some(last) = last.filter(|l| *l != from) {
            let scene = documents.remove(from);
            let at = if last > from { last } else { last + 1 };
            documents.insert(at, scene);
        }
        return Ok(());
    }
    let mut scene = documents.remove(from);
    if let Some(object) = scene.as_object_mut() {
        object.insert("chapterId", JsValue::str(chapter));
    }
    let at = documents
        .iter()
        .rposition(|d| chapter_of(d) == chapter)
        .map_or(documents.len(), |i| i + 1);
    documents.insert(at, scene);
    Ok(())
}

/// `permanentlyDelete`: a trashed document gone for good.
pub fn delete_document(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    list_mut(project, "documents")?.retain(|d| !has_id(d, id));
    Ok(())
}

/// `permanentlyDeleteChapter`: a trashed chapter with all its scenes, their storyboard cards and
/// the chapter's goal, gone for good.
pub fn delete_chapter(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    list_mut(project, "chapters")?.retain(|c| !has_id(c, id));
    let documents = list_mut(project, "documents")?;
    let scenes: Vec<String> = documents
        .iter()
        .filter(|d| chapter_of(d) == id)
        .map(|d| text(d.get("id")))
        .collect();
    documents.retain(|d| chapter_of(d) != id);
    if let Ok(cards) = list_mut(project, "storyboardCards") {
        cards.retain(|card| {
            let kind = text(card.get("kind"));
            let source = text(card.get("sourceId"));
            !(kind == "chapter" && source == id || kind == "scene" && scenes.contains(&source))
        });
    }
    if let Some(goals) = project
        .as_object_mut()
        .and_then(|p| p.get_mut("chapterGoals"))
        .and_then(JsValue::as_object_mut)
    {
        goals.remove(id);
    }
    Ok(())
}

/// `addMatterPage`: a new front (`frontMatter`) or back (`backMatter`) matter page from a
/// preset, at the end, while there are fewer than 12.
pub fn add_matter_page(
    project: &mut JsValue,
    key: &str,
    id: &str,
    title: &str,
    blank: bool,
) -> Result<(), EditError> {
    let options = project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .get_mut("publishOptions")
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?;
    let mut pages = options
        .get(key)
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    if pages.len() >= 12 {
        return Ok(());
    }
    let mut page = JsObject::new();
    page.insert("id", JsValue::str(&format!("matter-{id}")));
    page.insert(
        "title",
        JsValue::str(&title.chars().take(120).collect::<String>()),
    );
    page.insert("body", JsValue::str(""));
    if blank {
        page.insert("blank", JsValue::Bool(true));
    }
    pages.push(JsValue::Object(page));
    options.insert(key, JsValue::Array(pages));
    Ok(())
}

fn publish_options_mut(project: &mut JsValue) -> Result<&mut JsObject, EditError> {
    let project = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(project.get("publishOptions"), Some(JsValue::Object(_))) {
        project.insert("publishOptions", JsValue::Object(JsObject::new()));
    }
    project
        .get_mut("publishOptions")
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)
}

/// `saveStyleDraft`: a custom style saved (a new one, with `custom-` and `unique` as its id,
/// while there are fewer than 24) and chosen.
pub fn save_custom_style(
    project: &mut JsValue,
    style: &crate::publish::styles::CustomStyle,
    unique: &str,
) -> Result<(), EditError> {
    use crate::publish::styles::{ID_PREFIX, MAX_STYLES, normalize_draft, to_value};
    let mut saved = normalize_draft(style);
    if saved.id.is_empty() {
        saved.id = format!("{ID_PREFIX}{unique}");
    }
    let options = publish_options_mut(project)?;
    let mut styles = options
        .get("customStyles")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    match styles.iter().position(|s| text(s.get("id")) == saved.id) {
        Some(index) => styles[index] = to_value(&saved),
        None if styles.len() < MAX_STYLES => styles.push(to_value(&saved)),
        None => return Ok(()),
    }
    options.insert("customStyles", JsValue::Array(styles));
    options.insert("style", JsValue::str(&saved.id));
    Ok(())
}

/// `deleteStyleDraft`: a custom style gone; Manuscript chosen if it was.
pub fn delete_custom_style(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    let options = publish_options_mut(project)?;
    let styles: Vec<JsValue> = options
        .get("customStyles")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|s| text(s.get("id")) != id)
        .cloned()
        .collect();
    options.insert("customStyles", JsValue::Array(styles));
    if text(options.get("style")) == id {
        options.insert("style", JsValue::str("manuscript"));
    }
    Ok(())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\u{a0}', "&nbsp;")
}

/// The HTML for plain paragraphs, one per line (Enter starts a new paragraph, as in ProseCraft's
/// editor), as ProseCraft writes them; an empty line is its empty paragraph, `<p><br></p>`.
pub fn paragraphs_to_html(text: &str) -> String {
    let paragraphs: Vec<&str> = text
        .split('\n')
        .map(|p| p.strip_suffix('\r').unwrap_or(p))
        .collect();
    if paragraphs.iter().all(|p| p.is_empty()) {
        return "<p><br></p>".to_owned();
    }
    paragraphs
        .iter()
        .map(|p| {
            if p.is_empty() {
                "<p><br></p>".to_owned()
            } else {
                format!("<p>{}</p>", escape(p))
            }
        })
        .collect()
}

/// The scene's text as plain paragraphs, if that is all it is: only `<p>` elements holding text
/// (or ProseCraft's empty `<p><br></p>`), and writing the paragraphs back gives exactly the same
/// HTML. Anything else (formatting, footnotes, images, lists) is `None`, so a plain editor can
/// never drop it.
pub fn plain_paragraphs(html: &str) -> Option<String> {
    let mut paragraphs = Vec::new();
    for node in parse_fragment(html) {
        let Node::Element(p) = node else { return None };
        if p.name != "p" || !p.attrs.is_empty() {
            return None;
        }
        let text = match p.children.as_slice() {
            [] => String::new(),
            [Node::Element(br)]
                if br.name == "br" && br.attrs.is_empty() && br.children.is_empty() =>
            {
                String::new()
            }
            [Node::Text(t)] if !t.contains('\n') && !t.contains('\r') => t.clone(),
            _ => return None,
        };
        paragraphs.push(text);
    }
    let text = paragraphs.join("\n");
    (paragraphs_to_html(&text) == html).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::{
        add_chapter, chapter_label, paragraphs_to_html, plain_paragraphs, restore_document,
        trash_chapter,
    };
    use crate::js::{self, JsValue};

    fn project() -> JsValue {
        js::parse(
            r#"{"id":"p","name":"N","author":"","activeDocumentId":"","projectGoal":0,"chapterGoals":{},"activity":{},
            "chapters":[{"id":"c1","title":"Chapter 1","expanded":true}],
            "documents":[{"id":"s1","title":"One","content":"<p>a</p>","chapterId":"c1"},{"id":"w","title":"Wiki","content":"","space":"Wiki","chapterId":"c1"}],
            "future":{"kept":true}}"#,
        )
        .unwrap_or(JsValue::Null)
    }

    #[test]
    fn new_chapters_number_themselves_and_bring_a_scene() {
        let mut p = project();
        let (chapter, scene) = add_chapter(&mut p, "c2".into(), "s2".into()).unwrap_or_default();
        assert_eq!((chapter.as_str(), scene.as_str()), ("c2", "s2"));
        let text = js::stringify(&p);
        assert!(
            text.contains(r#"{"id":"c2","title":"Chapter 2","expanded":true,"summary":""}"#),
            "{text}"
        );
        assert!(
            text.contains(
                r#"{"id":"s2","title":"Scene 1","content":"<p><br></p>","chapterId":"c2"}"#
            ),
            "{text}"
        );
        assert!(text.contains(r#""future":{"kept":true}"#));
    }

    #[test]
    fn a_trashed_chapter_takes_its_scenes_and_restoring_one_brings_it_back() {
        let mut p = project();
        assert!(trash_chapter(&mut p, "c1").is_ok());
        let text = js::stringify(&p);
        assert!(
            text.contains(r#""trashed":true,"trashedWithChapterId":"c1""#),
            "{text}"
        );
        assert!(
            !text.contains(r#""space":"Wiki","chapterId":"c1","trashed""#),
            "the Wiki page stays: {text}"
        );
        assert!(restore_document(&mut p, "s1").is_ok());
        let text = js::stringify(&p);
        assert!(!text.contains("trashedWithChapterId"), "{text}");
        assert!(
            text.contains(r#"{"id":"c1","title":"Chapter 1","expanded":true,"trashed":false}"#),
            "{text}"
        );
    }

    #[test]
    fn only_plain_paragraphs_are_editable_as_text() {
        assert_eq!(
            plain_paragraphs("<p>One.</p><p>Two &amp; three.</p>").as_deref(),
            Some("One.\nTwo & three.")
        );
        assert_eq!(plain_paragraphs("<p><br></p>").as_deref(), Some(""));
        for rich in [
            "<p>It was <em>dark</em>.</p>",
            "<h1>Title</h1>",
            "<p data-sp=\"action\">x</p>",
            "<ul><li>a</li></ul>",
            "text",
            "<p>a<br>b</p>",
            "<p>a</p>\n<p>b</p>",
        ] {
            assert_eq!(plain_paragraphs(rich), None, "{rich}");
        }
        for text in [
            "",
            "One.",
            "One.\nTwo < three & four",
            "One.\n\nAfter an empty paragraph",
            "\u{a0}spaced",
        ] {
            let html = paragraphs_to_html(text);
            assert_eq!(plain_paragraphs(&html).as_deref(), Some(text), "{html}");
        }
    }

    #[test]
    fn chapter_labels_follow_their_place() {
        assert_eq!(
            chapter_label("Chapter 7", &["Prologue", "Chapter 3", "Chapter 7"]),
            "Chapter 2"
        );
        assert_eq!(chapter_label("Prologue", &["Prologue"]), "Prologue");
        assert_eq!(chapter_label("Chapter ٣", &["Chapter ٣"]), "Chapter ٣");
    }
}

#[cfg(test)]
mod goal_tests {
    use super::{set_chapter_goal, set_project_goal, track_activity};
    use crate::js::{parse, stringify};

    #[test]
    fn goals_and_activity_as_prosecraft_keeps_them() {
        let mut p = parse(r#"{"projectGoal":1,"chapterGoals":{"a":5},"activity":{"2026-11-01":{"edits":2,"wordsAdded":3}}}"#).expect("json");
        set_project_goal(&mut p, 1234.9).expect("goal");
        set_chapter_goal(&mut p, "b", -3.0).expect("goal");
        set_chapter_goal(&mut p, "a", f64::NAN).expect("goal");
        track_activity(&mut p, "2026-11-01", 4.0, 10.0).expect("activity");
        track_activity(&mut p, "2026-11-02", 1.0, -5.0).expect("activity");
        assert_eq!(
            stringify(&p),
            r#"{"projectGoal":1234,"chapterGoals":{"a":0,"b":0},"activity":{"2026-11-01":{"edits":6,"wordsAdded":13},"2026-11-02":{"edits":1,"wordsAdded":0}}}"#
        );
    }
}

#[cfg(test)]
mod markdown_tests {
    use super::{set_document_content, set_document_markdown};
    use crate::js::{self, JsValue};

    /// `applySingleDocumentMarkdown`: the content and source kept in their places, the
    /// footnotes replaced (and put last, as the object spread puts them), or dropped when the
    /// Markdown has none; a rich edit then drops the source.
    #[test]
    fn markdown_is_applied_as_prosecraft_applies_it() {
        let mut project = js::parse(
            r#"{"documents":[{"id":"d","title":"T","content":"<p>old</p>","footnotes":[{"id":"x","html":"old"}],"chapterId":"c","labelId":"l"}]}"#,
        )
        .unwrap_or(JsValue::Null);
        let source = "New *text*[^n1] <u>u</u>\n\n[^n1]: A **note**.";
        assert!(set_document_markdown(&mut project, "d", source).is_ok());
        assert_eq!(
            js::stringify(&project),
            r#"{"documents":[{"id":"d","title":"T","content":"<p>New <em>text</em><sup class=\"footnote-ref\" data-footnote-id=\"n1\" contenteditable=\"false\"></sup> <u>u</u></p>\n","chapterId":"c","labelId":"l","markdownSource":"New *text*[^n1] <u>u</u>\n\n[^n1]: A **note**.","footnotes":[{"id":"n1","html":"A <strong>note</strong>."}]}]}"#
        );
        assert!(set_document_markdown(&mut project, "d", "Plain").is_ok());
        assert_eq!(
            js::stringify(&project),
            r#"{"documents":[{"id":"d","title":"T","content":"<p>Plain</p>\n","chapterId":"c","labelId":"l","markdownSource":"Plain"}]}"#
        );
        assert!(set_document_content(&mut project, "d", "<p>Rich</p>").is_ok());
        assert_eq!(
            js::stringify(&project),
            r#"{"documents":[{"id":"d","title":"T","content":"<p>Rich</p>","chapterId":"c","labelId":"l"}]}"#
        );
    }
}

#[cfg(test)]
mod moves {
    use super::*;
    use crate::js::{parse, stringify};

    fn project() -> JsValue {
        parse(r#"{"chapters":[{"id":"a"},{"id":"t","trashed":true},{"id":"b"}],
            "documents":[{"id":"1","chapterId":"a"},{"id":"2","chapterId":"a"},{"id":"x","chapterId":"a","trashed":true},{"id":"3","chapterId":"b"}],
            "storyboardCards":[{"id":"c1","kind":"chapter","sourceId":"a"},{"id":"c2","kind":"scene","sourceId":"2"},{"id":"c3","kind":"scene","sourceId":"3"}],
            "chapterGoals":{"a":100,"b":5}}"#)
        .expect("json")
    }

    fn ids(p: &JsValue, key: &str) -> String {
        p.get(key)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|d| text(d.get("id")))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn chapters_move_among_the_live_ones() {
        let mut p = project();
        move_chapter(&mut p, "a", 1).expect("moved");
        assert_eq!(ids(&p, "chapters"), "b,t,a");
        move_chapter(&mut p, "a", 1).expect("no further");
        assert_eq!(ids(&p, "chapters"), "b,t,a");
        move_chapter(&mut p, "a", -1).expect("back");
        assert_eq!(ids(&p, "chapters"), "a,t,b");
    }

    #[test]
    fn scenes_move_within_their_chapter_and_to_another() {
        let mut p = project();
        move_scene(&mut p, "2", -1).expect("moved");
        assert_eq!(ids(&p, "documents"), "2,1,x,3");
        move_scene(&mut p, "2", -1).expect("first already");
        assert_eq!(ids(&p, "documents"), "2,1,x,3");
        move_scene_to_chapter_end(&mut p, "2", "b").expect("to b");
        assert_eq!(ids(&p, "documents"), "1,x,3,2");
        assert_eq!(
            p.get("documents")
                .and_then(JsValue::as_array)
                .and_then(|d| d.last())
                .map(chapter_of)
                .as_deref(),
            Some("b")
        );
        // To the end of its own chapter.
        move_scene_to_chapter_end(&mut p, "3", "b").expect("own");
        assert_eq!(ids(&p, "documents"), "1,x,2,3");
        // Not into a trashed chapter.
        move_scene_to_chapter_end(&mut p, "1", "t").expect("ignored");
        assert_eq!(ids(&p, "documents"), "1,x,2,3");
    }

    #[test]
    fn deleting_for_good_takes_scenes_cards_and_goal() {
        let mut p = project();
        delete_document(&mut p, "x").expect("deleted");
        assert_eq!(ids(&p, "documents"), "1,2,3");
        delete_chapter(&mut p, "a").expect("deleted");
        assert_eq!(ids(&p, "chapters"), "t,b");
        assert_eq!(ids(&p, "documents"), "3");
        assert_eq!(ids(&p, "storyboardCards"), "c3");
        assert_eq!(
            stringify(p.get("chapterGoals").expect("goals")),
            r#"{"b":5}"#
        );
    }
}

#[cfg(test)]
mod tile_tests {
    use super::patch_tile_document;
    use crate::js::{parse, stringify};

    fn project() -> crate::js::JsValue {
        parse(
            r#"{"chapters":[],"documents":[{"id":"scene","title":"Scene 1","content":"<p>one</p>","markdownSource":"one"},{"id":"wiki","title":"Castle","content":"<p>two</p>","space":"Wiki"}]}"#,
        )
        .expect("json")
    }

    fn docs(p: &crate::js::JsValue) -> String {
        stringify(p.get("documents").expect("documents"))
    }

    #[test]
    fn tile_edits_patch_one_document_as_prosecraft_does() {
        // Renames one document and leaves the others in place.
        let mut p = project();
        patch_tile_document(&mut p, "scene", Some("Opening"), None).expect("patch");
        assert_eq!(
            docs(&p),
            r#"[{"id":"scene","title":"Opening","content":"<p>one</p>","markdownSource":"one"},{"id":"wiki","title":"Castle","content":"<p>two</p>","space":"Wiki"}]"#
        );
        // Drops the cached Markdown source when the body changes.
        let mut p = project();
        patch_tile_document(&mut p, "scene", None, Some("<p>one more</p>")).expect("patch");
        assert!(
            docs(&p)
                .starts_with(r#"[{"id":"scene","title":"Scene 1","content":"<p>one more</p>"},"#),
            "{}",
            docs(&p)
        );
        // Keeps it when the body is unchanged.
        let mut p = project();
        patch_tile_document(&mut p, "scene", None, Some("<p>one</p>")).expect("patch");
        assert_eq!(docs(&p), docs(&project()));
        // Edits wiki pages like any other document.
        let mut p = project();
        patch_tile_document(&mut p, "wiki", Some("Castle Keep"), Some("<p>three</p>"))
            .expect("patch");
        assert!(
            docs(&p).ends_with(
                r#"{"id":"wiki","title":"Castle Keep","content":"<p>three</p>","space":"Wiki"}]"#
            ),
            "{}",
            docs(&p)
        );
        // An unknown id changes nothing.
        let mut p = project();
        patch_tile_document(&mut p, "missing", Some("Nope"), None).expect("patch");
        assert_eq!(docs(&p), docs(&project()));
    }

    #[test]
    fn a_scene_pane_writes_its_chapter_back_to_the_scenes() {
        let mut p = parse(
            r#"{"chapters":[{"id":"c"}],"documents":[{"id":"a","title":"A","content":"<p>one</p>","chapterId":"c"},{"id":"b","title":"B","content":"<p>two</p>","chapterId":"c"}]}"#,
        )
        .expect("json");
        let chapter = crate::chapter::build_chapter_html(
            &["<p>one!</p>".to_owned(), "<p>two!</p>".to_owned()],
            crate::publish::SceneSeparator::Hash,
        );
        patch_tile_document(&mut p, "b", None, Some(&chapter)).expect("patch");
        let out = docs(&p);
        assert!(
            out.contains(r#""id":"a","title":"A","content":"<p>one!</p>""#)
                && out.contains(r#""id":"b","title":"B","content":"<p>two!</p>""#),
            "{out}"
        );
    }
}

#[cfg(test)]
mod drag_tests {
    use super::{move_scene_to_position, reorder_chapter};
    use crate::js::{parse, stringify};

    fn ids(p: &crate::js::JsValue, key: &str) -> String {
        p.get(key)
            .and_then(crate::js::JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|d| {
                let id = stringify(d.get("id").unwrap_or(&crate::js::JsValue::Null));
                let chapter = d.get("chapterId").map(stringify).unwrap_or_default();
                format!(
                    "{}{}",
                    id.trim_matches('"'),
                    if chapter.is_empty() {
                        String::new()
                    } else {
                        format!("@{}", chapter.trim_matches('"'))
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn chapters_reorder_around_the_trash_as_reorderchapter_does() {
        let mut p =
            parse(r#"{"chapters":[{"id":"a"},{"id":"t","trashed":true},{"id":"b"},{"id":"c"}]}"#)
                .expect("json");
        reorder_chapter(&mut p, "c", "a", false).expect("move");
        assert_eq!(ids(&p, "chapters"), "c t a b");
        reorder_chapter(&mut p, "c", "b", true).expect("move");
        assert_eq!(ids(&p, "chapters"), "a t b c");
        reorder_chapter(&mut p, "a", "a", true).expect("same");
        reorder_chapter(&mut p, "a", "missing", true).expect("missing");
        assert_eq!(ids(&p, "chapters"), "a t b c");
    }

    #[test]
    fn scenes_move_before_or_after_another_and_change_chapter() {
        let mut p = parse(
            r#"{"documents":[{"id":"1","chapterId":"x"},{"id":"2","chapterId":"x"},{"id":"w","space":"Wiki"},{"id":"3","chapterId":"y"}]}"#,
        )
        .expect("json");
        move_scene_to_position(&mut p, "1", "2", true).expect("move");
        assert_eq!(ids(&p, "documents"), "2@x 1@x w 3@y");
        move_scene_to_position(&mut p, "1", "3", false).expect("move");
        assert_eq!(ids(&p, "documents"), "2@x w 1@y 3@y");
        // Onto a Wiki page (no chapter): nothing moves.
        move_scene_to_position(&mut p, "2", "w", true).expect("move");
        assert_eq!(ids(&p, "documents"), "2@x w 1@y 3@y");
    }
}
