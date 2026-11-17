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
