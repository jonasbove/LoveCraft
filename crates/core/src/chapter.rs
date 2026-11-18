//! A chapter edited as one document: its scenes joined with scene dividers (ProseCraft
//! `src/lib/chapter-sections.ts`, and `chapterMarkdownSource` / `writeChapterSectionsFor` in
//! `src/routes/+page.svelte`, upstream `2d1cf18`).
//!
//! ProseCraft's editor shows every scene of the open scene's chapter in one document. Scenes stay
//! separate in storage: the chapter's HTML is split at its dividers (or its Markdown at marker
//! lines) and each section is written back to a scene.

use crate::edit::EditError;
use crate::html::{Element, Node, is_js_whitespace, parse_fragment, sanitize_html, serialize};
use crate::js::{JsObject, JsValue};
use crate::markdown::{FootnoteDef, html_to_markdown};
use crate::publish::SceneSeparator;

/// Class marking a scene divider block in chapter HTML.
pub const SCENE_BREAK_CLASS: &str = "scene-break";
/// Markdown marker line separating scenes in chapter source.
pub const SCENE_BREAK_MARKER: &str = "<!-- scene-break -->";
/// The empty paragraph ProseCraft's editor writes.
pub const EMPTY_SECTION: &str = "<p><br></p>";

const MEDIA: [&str; 6] = ["img", "svg", "video", "audio", "canvas", "input"];
/// `RICH_SELECTOR`: inline and display math, diagrams and calculation worksheets.
const RICH_CLASSES: [&str; 4] = [
    "math-inline",
    "math-block",
    "mermaid-diagram",
    "calc-worksheet",
];

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `sceneBreakGlyph`: the glyph shown inside a divider for the publish separator setting.
pub fn scene_break_glyph(separator: SceneSeparator) -> &'static str {
    match separator {
        SceneSeparator::Hash => "#",
        SceneSeparator::Asterisks => "* * *",
        _ => "",
    }
}

/// `sceneBreakHtml`: a divider block, atomic in the editor.
pub fn scene_break_html(separator: SceneSeparator) -> String {
    let extra = match separator {
        SceneSeparator::Line => " scene-break-line",
        SceneSeparator::Blank | SceneSeparator::None => " scene-break-blank",
        _ => "",
    };
    let inner = if separator == SceneSeparator::Line {
        String::new()
    } else {
        scene_break_glyph(separator)
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!(r#"<div class="{SCENE_BREAK_CLASS}{extra}" contenteditable="false">{inner}</div>"#)
}

/// `isSceneBreakElement`: a `<div>` with the divider class.
pub fn is_scene_break(element: &Element) -> bool {
    element.name == "div" && element.has_class(SCENE_BREAK_CLASS)
}

/// `buildChapterHtml`: the sections joined with dividers; an empty section becomes an empty
/// paragraph so the caret always has a home.
pub fn build_chapter_html(sections: &[String], separator: SceneSeparator) -> String {
    if sections.is_empty() {
        return EMPTY_SECTION.to_owned();
    }
    let divider = scene_break_html(separator);
    sections
        .iter()
        .map(|s| {
            if js_trim(s).is_empty() {
                EMPTY_SECTION
            } else {
                s.as_str()
            }
        })
        .collect::<Vec<_>>()
        .join(&divider)
}

fn drop_nested_dividers(element: &mut Element) {
    element.children.retain(|child| match child {
        Node::Element(e) => !e.has_class(SCENE_BREAK_CLASS),
        Node::Text(_) => true,
    });
    for child in &mut element.children {
        if let Node::Element(e) = child {
            drop_nested_dividers(e);
        }
    }
}

/// `splitChapterHtml`: the chapter's sections, split at top-level dividers. Dividers nested
/// deeper (from pastes) are dropped; each section keeps its order.
pub fn split_chapter_html(html: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current: Vec<Node> = Vec::new();
    for child in parse_fragment(html) {
        match child {
            Node::Element(e) if is_scene_break(&e) => {
                sections.push(serialize(&current));
                current.clear();
            }
            Node::Element(mut e) => {
                drop_nested_dividers(&mut e);
                current.push(Node::Element(e));
            }
            text => current.push(text),
        }
    }
    sections.push(serialize(&current));
    sections
}

fn text_content(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => text_content(&e.children, out),
        }
    }
}

fn has_media_or_rich(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element(e) => {
            MEDIA.contains(&e.name.as_str())
                || RICH_CLASSES.iter().any(|c| e.has_class(c))
                || has_media_or_rich(&e.children)
        }
        Node::Text(_) => false,
    })
}

/// `sectionIsBlank`: the section holds no text, media or rich widgets.
pub fn section_is_blank(html: &str) -> bool {
    let nodes = parse_fragment(html);
    let mut text = String::new();
    text_content(&nodes, &mut text);
    js_trim(&text).is_empty() && !has_media_or_rich(&nodes)
}

/// `^[ \t]*<!--[ \t]*scene-break[ \t]*-->[ \t]*$`.
fn is_marker_line(line: &str) -> bool {
    let tabs = |c: char| c == ' ' || c == '\t';
    let Some(rest) = line.trim_matches(tabs).strip_prefix("<!--") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix("-->") else {
        return false;
    };
    rest.trim_matches(tabs) == "scene-break"
}

/// `splitChapterMarkdown`: the chapter source's sections, split at marker lines, without
/// leading or trailing newlines.
pub fn split_chapter_markdown(source: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let finish = |lines: &[&str]| lines.join("\n").trim_matches('\n').to_owned();
    for line in source.split('\n') {
        if is_marker_line(line) {
            sections.push(finish(&current));
            current.clear();
            continue;
        }
        current.push(line);
    }
    sections.push(finish(&current));
    sections
}

/// `joinChapterMarkdown`: the sections joined with marker lines.
pub fn join_chapter_markdown(sections: &[String]) -> String {
    sections.join(&format!("\n\n{SCENE_BREAK_MARKER}\n\n"))
}

fn text(value: Option<&JsValue>) -> Option<String> {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

fn footnote_defs(value: Option<&JsValue>) -> Vec<FootnoteDef> {
    value
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            Some(FootnoteDef {
                id: text(f.get("id"))?,
                html: text(f.get("html")).unwrap_or_default(),
            })
        })
        .collect()
}

fn documents(project: &JsValue) -> &[JsValue] {
    project
        .get("documents")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

fn in_chapter(doc: &JsValue, chapter_id: &str) -> bool {
    doc.get("chapterId")
        .and_then(JsValue::as_js_string)
        .is_some_and(|c| c.eq_str(chapter_id))
}

fn trashed(doc: &JsValue) -> bool {
    matches!(doc.get("trashed"), Some(JsValue::Bool(true)))
}

/// `chapterMarkdownSource`: the chapter's live scenes as one Markdown source, from their own
/// sources when every scene has one, else from their HTML.
pub fn chapter_markdown_source(project: &JsValue, chapter_id: &str) -> String {
    let docs: Vec<&JsValue> = documents(project)
        .iter()
        .filter(|d| in_chapter(d, chapter_id) && !trashed(d))
        .collect();
    let sources: Option<Vec<String>> = docs.iter().map(|d| text(d.get("markdownSource"))).collect();
    match sources {
        Some(sources) if !docs.is_empty() => join_chapter_markdown(&sources),
        _ => join_chapter_markdown(
            &docs
                .iter()
                .map(|d| {
                    html_to_markdown(
                        &text(d.get("content")).unwrap_or_default(),
                        &footnote_defs(d.get("footnotes")),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    }
}

fn footnote_id(def: &JsValue) -> Option<String> {
    text(def.get("id"))
}

fn footnote_list(doc: &JsValue) -> Vec<JsValue> {
    doc.get("footnotes")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default()
}

/// What a scene gets from a chapter save.
struct Update {
    content: String,
    markdown: Option<String>,
    footnotes: Option<Vec<JsValue>>,
}

/// `writeChapterSectionsFor`: the chapter's sections written back to its scenes in order.
///
/// - Surplus sections fold into the last scene, first reviving trashed scenes of the chapter
///   (the undo of a merge).
/// - Scenes left without a section are emptied and moved to the trash.
/// - From the rich editor (`markdown` is `None`): each scene's Markdown source is dropped, and a
///   footnote definition moves to the scene that now holds its marker.
/// - From the Markdown editor: each scene takes its section's source and definitions.
///
/// Sections are assigned by position, as in ProseCraft.
pub fn write_chapter_sections(
    project: &mut JsValue,
    chapter_id: &str,
    sections: &[String],
    markdown: Option<&[String]>,
    footnotes: Option<&[Vec<JsValue>]>,
) -> Result<(), EditError> {
    let docs = documents(project);
    let live: Vec<usize> = (0..docs.len())
        .filter(|&i| in_chapter(&docs[i], chapter_id) && !trashed(&docs[i]))
        .collect();
    if live.is_empty() {
        return Ok(());
    }
    let dead: Vec<usize> = (0..docs.len())
        .filter(|&i| in_chapter(&docs[i], chapter_id) && trashed(&docs[i]))
        .collect();
    let mut targets = live.clone();
    targets.extend(
        dead.iter()
            .copied()
            .take(sections.len().saturating_sub(live.len())),
    );

    let mut merged: Vec<String> = sections.to_vec();
    let mut merged_markdown: Option<Vec<String>> = markdown.map(<[String]>::to_vec);
    let mut merged_footnotes: Option<Vec<Vec<JsValue>>> = footnotes.map(<[Vec<JsValue>]>::to_vec);
    while merged.len() > targets.len() {
        let extra = merged.pop().unwrap_or_default();
        if let Some(last) = merged.last_mut() {
            last.push_str(&extra);
        }
        if let Some(md) = merged_markdown.as_mut() {
            let extra = md.pop().unwrap_or_default();
            if let Some(last) = md.last_mut() {
                *last = format!("{last}\n\n{extra}");
            }
        }
        if let Some(notes) = merged_footnotes.as_mut() {
            let extra = notes.pop().unwrap_or_default();
            if let Some(tail) = notes.last_mut() {
                for note in extra {
                    let id = footnote_id(&note);
                    if !tail.iter().any(|n| footnote_id(n) == id) {
                        tail.push(note);
                    }
                }
            }
        }
    }

    let mut updates: Vec<Option<Update>> = Vec::new();
    for index in 0..targets.len() {
        updates.push((index < merged.len()).then(|| {
            Update {
                content: sanitize_html(&merged[index]),
                markdown: merged_markdown
                    .as_ref()
                    .map(|md| md.get(index).cloned().unwrap_or_default()),
                footnotes: merged_footnotes
                    .as_ref()
                    .map(|n| n.get(index).cloned().unwrap_or_default()),
            }
        }));
    }

    // From the rich editor, a definition goes with its marker when a split or merge moved it
    // to another scene.
    let mut reassigned: Vec<Option<Vec<JsValue>>> = vec![None; targets.len()];
    if merged_footnotes.is_none() {
        let mut owners: Vec<(String, usize, JsValue)> = Vec::new();
        for (slot, &doc) in targets.iter().enumerate() {
            for def in footnote_list(&docs[doc]) {
                if let Some(id) = footnote_id(&def)
                    && !owners.iter().any(|(o, _, _)| *o == id)
                {
                    owners.push((id, slot, def));
                }
            }
        }
        let refs: Vec<Option<Vec<String>>> = updates
            .iter()
            .map(|u| {
                u.as_ref()
                    .map(|u| crate::footnotes::extract_ids(&u.content))
            })
            .collect();
        let first_ref = |id: &str| {
            refs.iter()
                .position(|r| r.as_ref().is_some_and(|ids| ids.iter().any(|i| i == id)))
        };
        for (slot, update) in updates.iter().enumerate() {
            if update.is_some() {
                reassigned[slot] = Some(Vec::new());
            }
        }
        for (id, owner, def) in owners {
            let current = if updates[owner].is_some() { owner } else { 0 };
            let stays = refs[current].as_ref().is_some_and(|ids| ids.contains(&id));
            let dest = if stays {
                current
            } else {
                first_ref(&id).unwrap_or(current)
            };
            if let Some(list) = reassigned[dest].as_mut() {
                list.push(def);
            }
        }
    }

    let list = project
        .as_object_mut()
        .and_then(|o| o.get_mut("documents"))
        .and_then(|d| match d {
            JsValue::Array(items) => Some(items),
            _ => None,
        })
        .ok_or(EditError::Shape)?;
    let chapter_docs: Vec<usize> = (0..list.len())
        .filter(|&i| in_chapter(&list[i], chapter_id))
        .collect();
    for i in chapter_docs {
        let slot = targets.iter().position(|&t| t == i);
        let was_trashed = trashed(&list[i]);
        let doc = list[i].as_object_mut().ok_or(EditError::Shape)?;
        match slot.and_then(|s| updates[s].take().map(|u| (s, u))) {
            Some((
                _,
                Update {
                    content,
                    markdown: Some(markdown),
                    footnotes,
                },
            )) => {
                doc.remove("footnotes");
                doc.insert("content", JsValue::str(&content));
                doc.insert("markdownSource", JsValue::str(&markdown));
                let notes = footnotes.unwrap_or_default();
                if !notes.is_empty() {
                    doc.insert("footnotes", JsValue::Array(notes));
                }
                doc.insert("trashed", JsValue::Bool(false));
            }
            Some((slot, update)) => {
                let notes = reassigned[slot]
                    .take()
                    .unwrap_or_else(|| footnote_list(&JsValue::Object(doc.clone())));
                let had_source = doc.get("markdownSource").is_some();
                doc.remove("markdownSource");
                doc.remove("footnotes");
                doc.insert("content", JsValue::str(&update.content));
                if had_source || was_trashed {
                    doc.insert("trashed", JsValue::Bool(false));
                }
                if !notes.is_empty() {
                    doc.insert("footnotes", JsValue::Array(notes));
                }
            }
            None if !was_trashed => {
                doc.remove("markdownSource");
                doc.remove("footnotes");
                doc.insert("content", JsValue::str(EMPTY_SECTION));
                doc.insert("trashed", JsValue::Bool(true));
            }
            None => {}
        }
    }
    Ok(())
}

fn documents_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    match project.as_object_mut().and_then(|o| o.get_mut("documents")) {
        Some(JsValue::Array(items)) => Ok(items),
        _ => Err(EditError::Shape),
    }
}

fn has_id(doc: &JsValue, id: &str) -> bool {
    doc.get("id")
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

/// `handleChapterEnter`: Enter on an empty paragraph starts a new scene. The scene keeps the
/// text before the paragraph (`before`), and a new "Untitled scene" right after it in the
/// chapter takes the text after (`after`), with the footnote definitions whose markers moved.
/// The scene's Markdown source is dropped, as every chapter save does.
pub fn split_scene(
    project: &mut JsValue,
    id: &str,
    before: &str,
    after: &str,
    new_id: String,
) -> Result<(), EditError> {
    let list = documents_mut(project)?;
    let at = list
        .iter()
        .position(|d| has_id(d, id))
        .ok_or_else(|| EditError::NoDocument(id.to_owned()))?;
    let chapter = text(list[at].get("chapterId")).ok_or(EditError::Shape)?;
    let after = sanitize_html(after);
    let moved = crate::footnotes::extract_ids(&after);
    let (moving, staying): (Vec<JsValue>, Vec<JsValue>) = footnote_list(&list[at])
        .into_iter()
        .partition(|def| footnote_id(def).is_some_and(|i| moved.contains(&i)));
    let doc = list[at].as_object_mut().ok_or(EditError::Shape)?;
    let had_source = doc.remove("markdownSource").is_some();
    doc.remove("footnotes");
    doc.insert("content", JsValue::str(&sanitize_html(before)));
    if had_source {
        doc.insert("trashed", JsValue::Bool(false));
    }
    if !staying.is_empty() {
        doc.insert("footnotes", JsValue::Array(staying));
    }
    let mut scene = JsObject::new();
    scene.insert("id", JsValue::str(&new_id));
    scene.insert("title", JsValue::str("Untitled scene"));
    scene.insert("content", JsValue::str(&after));
    scene.insert("chapterId", JsValue::str(&chapter));
    if !moving.is_empty() {
        scene.insert("footnotes", JsValue::Array(moving));
    }
    list.insert(at + 1, JsValue::Object(scene));
    Ok(())
}

/// Deleting the divider before a scene: the scene's text and footnote definitions join the
/// end of the previous live scene of its chapter, and the scene is emptied and trashed, as
/// ProseCraft trashes a scene left without a section. Returns the previous scene's id.
///
/// ProseCraft maps the remaining sections to scenes by position, so there the *last* scene of
/// the chapter is trashed and each later scene takes the next one's text (and keeps its own
/// title). Here the joined scene is the one trashed.
pub fn join_scene_to_previous(project: &mut JsValue, id: &str) -> Result<String, EditError> {
    let list = documents_mut(project)?;
    let at = list
        .iter()
        .position(|d| has_id(d, id))
        .ok_or_else(|| EditError::NoDocument(id.to_owned()))?;
    let chapter = text(list[at].get("chapterId")).ok_or(EditError::Shape)?;
    let previous = list[..at]
        .iter()
        .rposition(|d| in_chapter(d, &chapter) && !trashed(d))
        .ok_or(EditError::Shape)?;
    let content = text(list[at].get("content")).unwrap_or_default();
    let notes = footnote_list(&list[at]);
    let previous_id = text(list[previous].get("id")).unwrap_or_default();
    let doc = list[previous].as_object_mut().ok_or(EditError::Shape)?;
    let joined = format!("{}{content}", text(doc.get("content")).unwrap_or_default());
    let mut list_notes = footnote_list(&JsValue::Object(doc.clone()));
    for note in notes {
        let id = footnote_id(&note);
        if !list_notes.iter().any(|n| footnote_id(n) == id) {
            list_notes.push(note);
        }
    }
    let had_source = doc.remove("markdownSource").is_some();
    doc.remove("footnotes");
    doc.insert("content", JsValue::str(&sanitize_html(&joined)));
    if had_source {
        doc.insert("trashed", JsValue::Bool(false));
    }
    if !list_notes.is_empty() {
        doc.insert("footnotes", JsValue::Array(list_notes));
    }
    let doc = list[at].as_object_mut().ok_or(EditError::Shape)?;
    doc.remove("markdownSource");
    doc.remove("footnotes");
    doc.insert("content", JsValue::str(EMPTY_SECTION));
    doc.insert("trashed", JsValue::Bool(true));
    Ok(previous_id)
}

/// The chapter's live scenes' HTML, in order (`scenesInChapter(...).map(doc => doc.content)`).
pub fn chapter_sections(project: &JsValue, chapter_id: &str) -> Vec<String> {
    documents(project)
        .iter()
        .filter(|d| in_chapter(d, chapter_id) && !trashed(d))
        .map(|d| text(d.get("content")).unwrap_or_default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js;

    #[test]
    fn scene_break_html_follows_the_separator() {
        assert_eq!(
            scene_break_html(SceneSeparator::Hash),
            r#"<div class="scene-break" contenteditable="false">#</div>"#
        );
        assert!(scene_break_html(SceneSeparator::Asterisks).contains("* * *"));
        assert!(scene_break_html(SceneSeparator::Line).contains("scene-break-line"));
        assert!(scene_break_html(SceneSeparator::Blank).contains("scene-break-blank"));
        assert!(scene_break_html(SceneSeparator::None).contains("scene-break-blank"));
        assert!(scene_break_html(SceneSeparator::Hash).contains(r#"contenteditable="false""#));
    }

    #[test]
    fn chapter_html_round_trips() {
        let sections: Vec<String> = ["<p>One</p>", "<p>Two</p><p>Three</p>", "<h2>Four</h2>"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            split_chapter_html(&build_chapter_html(&sections, SceneSeparator::Hash)),
            sections
        );
        let split = split_chapter_html(&build_chapter_html(
            &["<p>A</p>".into(), String::new(), "<p>C</p>".into()],
            SceneSeparator::Hash,
        ));
        assert_eq!(split.len(), 3);
        assert_eq!(split[1], EMPTY_SECTION);
        assert_eq!(split_chapter_html("<p>Lonely</p>"), ["<p>Lonely</p>"]);
        assert_eq!(build_chapter_html(&[], SceneSeparator::Hash), EMPTY_SECTION);
    }

    #[test]
    fn nested_dividers_are_dropped() {
        let html = format!(
            r#"<p>A</p><div>{}</div><div class="scene-break" contenteditable="false">#</div><p>B</p>"#,
            scene_break_html(SceneSeparator::Hash)
        );
        assert_eq!(
            split_chapter_html(&html),
            ["<p>A</p><div></div>", "<p>B</p>"]
        );
    }

    #[test]
    fn blank_sections() {
        assert!(section_is_blank(""));
        assert!(section_is_blank("<p><br></p>"));
        assert!(section_is_blank("<p>  </p>"));
        assert!(!section_is_blank("<p>Hi</p>"));
        assert!(!section_is_blank(r#"<p><img src="x"></p>"#));
        assert!(!section_is_blank(
            r#"<div class="math-block" data-tex="x"></div>"#
        ));
    }

    #[test]
    fn chapter_markdown_round_trips() {
        let sections: Vec<String> = ["# A\n\ntext", "more *text*", ""]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            split_chapter_markdown(&join_chapter_markdown(&sections)),
            sections
        );
        assert_eq!(
            split_chapter_markdown("a\n  <!-- scene-break -->  \nb"),
            ["a", "b"]
        );
        assert_eq!(
            split_chapter_markdown("a\n<!--scene-break-->x\nb"),
            ["a\n<!--scene-break-->x\nb"]
        );
        assert_eq!(split_chapter_markdown("\n\na\n\n"), ["a"]);
    }

    fn project(docs: &str) -> JsValue {
        js::parse(&format!(r#"{{"documents":{docs}}}"#)).expect("json")
    }

    #[test]
    fn markdown_source_uses_every_scene_source_or_none() {
        let p = project(
            r#"[{"id":"a","chapterId":"c","content":"<p>A</p>","markdownSource":"*A*"},{"id":"b","chapterId":"c","content":"<p>B</p>","markdownSource":"B"},{"id":"t","chapterId":"c","content":"<p>T</p>","trashed":true}]"#,
        );
        assert_eq!(
            chapter_markdown_source(&p, "c"),
            "*A*\n\n<!-- scene-break -->\n\nB"
        );
        let p = project(
            r#"[{"id":"a","chapterId":"c","content":"<p><em>A</em></p>","markdownSource":"*A*"},{"id":"b","chapterId":"c","content":"<p>B</p>"}]"#,
        );
        assert_eq!(
            chapter_markdown_source(&p, "c"),
            "_A_\n\n<!-- scene-break -->\n\nB"
        );
        assert_eq!(chapter_markdown_source(&project("[]"), "c"), "");
    }

    #[test]
    fn rich_sections_drop_sources_and_trash_scenes_without_a_section() {
        let mut p = project(
            r#"[{"id":"a","title":"A","chapterId":"c","content":"<p>A</p>","markdownSource":"A"},{"id":"x","chapterId":"other","content":"<p>X</p>"},{"id":"b","title":"B","chapterId":"c","content":"<p>B</p>"},{"id":"d","title":"D","chapterId":"c","content":"<p>D</p>","markdownSource":"D","footnotes":[{"id":"fn-1","html":"n"}]}]"#,
        );
        write_chapter_sections(
            &mut p,
            "c",
            &["<p>A</p><p>B</p>".into(), "<p>D</p>".into()],
            None,
            None,
        )
        .expect("write");
        assert_eq!(
            js::stringify(&p),
            r#"{"documents":[{"id":"a","title":"A","chapterId":"c","content":"<p>A</p><p>B</p>","trashed":false,"footnotes":[{"id":"fn-1","html":"n"}]},{"id":"x","chapterId":"other","content":"<p>X</p>"},{"id":"b","title":"B","chapterId":"c","content":"<p>D</p>"},{"id":"d","title":"D","chapterId":"c","content":"<p><br></p>","trashed":true}]}"#
        );
    }

    #[test]
    fn surplus_sections_revive_trashed_scenes_then_fold_into_the_last() {
        let mut p = project(
            r#"[{"id":"a","chapterId":"c","content":"<p>A</p>"},{"id":"t","chapterId":"c","content":"<p><br></p>","trashed":true}]"#,
        );
        let sections: Vec<String> = ["<p>1</p>", "<p>2</p>", "<p>3</p>"]
            .map(str::to_owned)
            .to_vec();
        write_chapter_sections(&mut p, "c", &sections, None, None).expect("write");
        assert_eq!(
            js::stringify(&p),
            r#"{"documents":[{"id":"a","chapterId":"c","content":"<p>1</p>"},{"id":"t","chapterId":"c","content":"<p>2</p><p>3</p>","trashed":false}]}"#
        );
    }

    #[test]
    fn a_footnote_follows_its_marker() {
        let marker = crate::footnotes::marker_html("fn-1");
        let mut p = project(&format!(
            r#"[{{"id":"a","chapterId":"c","content":"<p>A{m}</p>","footnotes":[{{"id":"fn-1","html":"n"}}]}},{{"id":"b","chapterId":"c","content":"<p>B</p>"}}]"#,
            m = marker.replace('"', "\\\"")
        ));
        let sections = vec!["<p>A</p>".to_owned(), format!("<p>B{marker}</p>")];
        write_chapter_sections(&mut p, "c", &sections, None, None).expect("write");
        let docs = documents(&p);
        assert!(docs[0].get("footnotes").is_none());
        assert_eq!(
            js::stringify(docs[1].get("footnotes").expect("moved")),
            r#"[{"id":"fn-1","html":"n"}]"#
        );
    }

    #[test]
    fn markdown_sections_set_sources_and_definitions() {
        let mut p = project(
            r#"[{"id":"a","chapterId":"c","content":"<p>A</p>","footnotes":[{"id":"fn-0","html":"old"}]},{"id":"b","chapterId":"c","content":"<p>B</p>"}]"#,
        );
        let note: Vec<JsValue> = vec![js::parse(r#"{"id":"fn-1","html":"n"}"#).expect("json")];
        write_chapter_sections(
            &mut p,
            "c",
            &["<p>A2</p>".into(), "<p>B2</p>".into(), "<p>C2</p>".into()],
            Some(&["A2".into(), "B2".into(), "C2".into()]),
            Some(&[Vec::new(), note.clone(), note]),
        )
        .expect("write");
        assert_eq!(
            js::stringify(&p),
            r#"{"documents":[{"id":"a","chapterId":"c","content":"<p>A2</p>","markdownSource":"A2","trashed":false},{"id":"b","chapterId":"c","content":"<p>B2</p><p>C2</p>","markdownSource":"B2\n\nC2","footnotes":[{"id":"fn-1","html":"n"}],"trashed":false}]}"#
        );
    }

    #[test]
    fn a_scene_splits_and_its_moved_footnotes_follow() {
        let marker = crate::footnotes::marker_html("fn-2");
        let mut p = project(
            r#"[{"id":"a","title":"A","chapterId":"c","content":"old","markdownSource":"old","footnotes":[{"id":"fn-1","html":"one"},{"id":"fn-2","html":"two"}]},{"id":"b","chapterId":"c","content":"<p>B</p>"}]"#,
        );
        split_scene(
            &mut p,
            "a",
            "<p>A</p>",
            &format!("<p>N{marker}</p>"),
            "n".into(),
        )
        .expect("split");
        let m = marker.replace('"', "\\\"");
        assert_eq!(
            js::stringify(&p),
            format!(
                r#"{{"documents":[{{"id":"a","title":"A","chapterId":"c","content":"<p>A</p>","trashed":false,"footnotes":[{{"id":"fn-1","html":"one"}}]}},{{"id":"n","title":"Untitled scene","content":"<p>N{m}</p>","chapterId":"c","footnotes":[{{"id":"fn-2","html":"two"}}]}},{{"id":"b","chapterId":"c","content":"<p>B</p>"}}]}}"#
            )
        );
    }

    #[test]
    fn a_scene_joins_the_previous_live_scene() {
        let mut p = project(
            r#"[{"id":"a","title":"A","chapterId":"c","content":"<p>A</p>","footnotes":[{"id":"fn-1","html":"one"}]},{"id":"t","chapterId":"c","content":"<p><br></p>","trashed":true},{"id":"x","chapterId":"other","content":"<p>X</p>"},{"id":"b","title":"B","chapterId":"c","content":"<p>B</p>","markdownSource":"B","footnotes":[{"id":"fn-2","html":"two"}]},{"id":"d","title":"D","chapterId":"c","content":"<p>D</p>"}]"#,
        );
        assert_eq!(join_scene_to_previous(&mut p, "b").expect("join"), "a");
        assert_eq!(
            js::stringify(&p),
            r#"{"documents":[{"id":"a","title":"A","chapterId":"c","content":"<p>A</p><p>B</p>","footnotes":[{"id":"fn-1","html":"one"},{"id":"fn-2","html":"two"}]},{"id":"t","chapterId":"c","content":"<p><br></p>","trashed":true},{"id":"x","chapterId":"other","content":"<p>X</p>"},{"id":"b","title":"B","chapterId":"c","content":"<p><br></p>","trashed":true},{"id":"d","title":"D","chapterId":"c","content":"<p>D</p>"}]}"#
        );
        assert!(join_scene_to_previous(&mut p, "a").is_err());
    }
}
