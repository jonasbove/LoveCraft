//! Footnotes in a scene (ProseCraft `src/lib/footnotes.ts` and the page's footnote handlers,
//! upstream `7dae544`): markers in the text (`<sup class="footnote-ref" data-footnote-id>`)
//! and their definitions in the document's `footnotes` (`{ id, html }`, inline HTML).

use crate::edit::EditError;
use crate::html::sanitize::{FOOTNOTE_REF_CLASS, is_valid_footnote_id, sanitize_inline_html};
use crate::html::{Node, parse_fragment, serialize};
use crate::js::{JsObject, JsValue};
use crate::project::Footnote;

const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `createFootnoteId`: `fn-` and six base-36 characters from `random` (0 ≤ r < 1).
pub fn create_footnote_id(mut random: impl FnMut() -> f64) -> String {
    let suffix: String = (0..6)
        .map(|_| {
            let r = random().clamp(0.0, 0.999_999_999);
            ID_ALPHABET[(r * ID_ALPHABET.len() as f64).floor() as usize] as char
        })
        .collect();
    format!("fn-{suffix}")
}

/// `footnoteMarkerHtml`.
pub fn marker_html(id: &str) -> String {
    format!(
        r#"<sup class="{FOOTNOTE_REF_CLASS}" data-footnote-id="{id}" contenteditable="false"></sup>"#
    )
}

fn is_marker(e: &crate::html::Element) -> bool {
    e.name == "sup" && e.has_class(FOOTNOTE_REF_CLASS) && e.attr("data-footnote-id").is_some()
}

fn collect_ids(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Element(e) = node {
            if is_marker(e) {
                let id = e.attr("data-footnote-id").unwrap_or_default();
                if is_valid_footnote_id(id) {
                    out.push(id.to_owned());
                }
            }
            collect_ids(&e.children, out);
        }
    }
}

/// `extractFootnoteIds`: the valid ids of the markers, in order (repeats kept).
pub fn extract_ids(html: &str) -> Vec<String> {
    if !html.contains(FOOTNOTE_REF_CLASS) {
        return Vec::new();
    }
    let mut ids = Vec::new();
    collect_ids(&parse_fragment(html), &mut ids);
    ids
}

/// `footnoteNumbers`: each referenced id once, in order (its number is its place + 1).
pub fn numbered_ids(html: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for id in extract_ids(html) {
        if !seen.contains(&id) {
            seen.push(id);
        }
    }
    seen
}

/// A referenced footnote: its number, and its definition's HTML (`None` when it has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberedFootnote {
    pub id: String,
    pub number: usize,
    pub html: Option<String>,
}

/// `numberedFootnotes`.
pub fn numbered(html: &str, defs: &[Footnote]) -> Vec<NumberedFootnote> {
    numbered_ids(html)
        .into_iter()
        .enumerate()
        .map(|(i, id)| NumberedFootnote {
            html: defs.iter().find(|d| d.id == id).map(|d| d.html.clone()),
            id,
            number: i + 1,
        })
        .collect()
}

/// `findOrphanFootnotes`: definitions without a marker.
pub fn orphans<'a>(html: &str, defs: &'a [Footnote]) -> Vec<&'a Footnote> {
    let referenced = numbered_ids(html);
    defs.iter()
        .filter(|d| !referenced.contains(&d.id))
        .collect()
}

fn strip_markers(nodes: Vec<Node>, id: &str, changed: &mut bool) -> Vec<Node> {
    nodes
        .into_iter()
        .filter_map(|node| match node {
            Node::Element(e) if is_marker(&e) && e.attr("data-footnote-id") == Some(id) => {
                *changed = true;
                None
            }
            Node::Element(mut e) => {
                e.children = strip_markers(std::mem::take(&mut e.children), id, changed);
                Some(Node::Element(e))
            }
            other => Some(other),
        })
        .collect()
}

/// `removeFootnoteRefFromHtml`: the HTML without `id`'s markers (unchanged when it has none).
pub fn remove_refs(html: &str, id: &str) -> String {
    if html.is_empty() || !is_valid_footnote_id(id) || !html.contains(id) {
        return html.to_owned();
    }
    let mut changed = false;
    let nodes = strip_markers(parse_fragment(html), id, &mut changed);
    if changed {
        serialize(&nodes)
    } else {
        html.to_owned()
    }
}

fn document_mut<'a>(project: &'a mut JsValue, id: &str) -> Result<&'a mut JsObject, EditError> {
    let Some(JsValue::Array(docs)) = project.as_object_mut().and_then(|o| o.get_mut("documents"))
    else {
        return Err(EditError::NoDocument(id.to_owned()));
    };
    docs.iter_mut()
        .find(|d| {
            d.get("id")
                .and_then(JsValue::as_js_string)
                .is_some_and(|v| v.eq_str(id))
        })
        .and_then(JsValue::as_object_mut)
        .ok_or_else(|| EditError::NoDocument(id.to_owned()))
}

fn definitions(doc: &JsObject) -> Vec<JsValue> {
    match doc.get("footnotes") {
        Some(JsValue::Array(list)) => list.clone(),
        _ => Vec::new(),
    }
}

/// Sets the definitions, leaving no empty list behind (as ProseCraft drops the key).
///
/// The Markdown editor's source holds the definitions (and the markers) as Markdown, and the
/// open sync folder writes that source and reads definitions back from it. A footnote edited
/// here would come back as it was on the next sync, so the source is dropped with every change
/// to a document's footnotes; the next time the Markdown editor opens, it starts from the
/// content. (ProseCraft keeps the source here, and loses such edits on its next open-folder
/// sync.)
fn set_definitions(doc: &mut JsObject, list: Vec<JsValue>) {
    doc.remove("markdownSource");
    if list.is_empty() {
        doc.remove("footnotes");
    } else {
        doc.insert("footnotes", JsValue::Array(list));
    }
}

fn def_id(d: &JsValue) -> Option<String> {
    d.get("id")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

/// `addFootnote`: an empty definition for `id`, last. (The marker is the editor's.)
pub fn add_definition(project: &mut JsValue, document: &str, id: &str) -> Result<(), EditError> {
    if !is_valid_footnote_id(id) {
        return Err(EditError::Shape);
    }
    let doc = document_mut(project, document)?;
    let mut list = definitions(doc);
    if list.iter().any(|d| def_id(d).as_deref() == Some(id)) {
        return Err(EditError::Shape);
    }
    let mut def = JsObject::new();
    def.insert("id", JsValue::str(id));
    def.insert("html", JsValue::str(""));
    list.push(JsValue::Object(def));
    set_definitions(doc, list);
    Ok(())
}

/// `updateFootnote`: the definition's HTML, sanitized as inline HTML.
pub fn update_definition(
    project: &mut JsValue,
    document: &str,
    id: &str,
    html: &str,
) -> Result<(), EditError> {
    let doc = document_mut(project, document)?;
    let clean = sanitize_inline_html(html);
    let mut list = definitions(doc);
    for d in list.iter_mut().filter(|d| def_id(d).as_deref() == Some(id)) {
        if let Some(o) = d.as_object_mut() {
            o.insert("html", JsValue::str(&clean));
        }
    }
    // As ProseCraft writes it (an empty list stays); the source goes, as in `set_definitions`.
    doc.remove("markdownSource");
    doc.insert("footnotes", JsValue::Array(list));
    Ok(())
}

/// `removeFootnoteById`: the markers and the definition.
pub fn remove(project: &mut JsValue, document: &str, id: &str) -> Result<(), EditError> {
    let doc = document_mut(project, document)?;
    if let Some(content) = doc
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
    {
        let next = remove_refs(&content, id);
        if next != content {
            doc.insert("content", JsValue::str(&next));
            doc.remove("markdownSource");
        }
    }
    let list = definitions(doc);
    if list.iter().any(|d| def_id(d).as_deref() == Some(id)) {
        set_definitions(
            doc,
            list.into_iter()
                .filter(|d| def_id(d).as_deref() != Some(id))
                .collect(),
        );
    }
    Ok(())
}

/// `cleanupOrphanFootnotes`: definitions whose marker is gone are dropped.
pub fn prune_orphans(project: &mut JsValue, document: &str) -> Result<(), EditError> {
    let doc = document_mut(project, document)?;
    let content = doc
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let referenced = numbered_ids(&content);
    let list = definitions(doc);
    if list.is_empty() {
        return Ok(());
    }
    set_definitions(
        doc,
        list.into_iter()
            .filter(|d| def_id(d).is_some_and(|id| referenced.contains(&id)))
            .collect(),
    );
    Ok(())
}

#[cfg(test)]
mod markdown_source_tests {
    use super::{add_definition, prune_orphans, remove, update_definition};
    use crate::js::{self, JsObject, JsValue};
    use crate::sync::open_layout::normalize_document;

    /// A scene last written in the Markdown editor (or read from an open sync folder).
    fn project() -> JsValue {
        let mut project =
            js::parse(r#"{"documents":[{"id":"d","title":"T","content":"","chapterId":"c"}]}"#)
                .unwrap_or(JsValue::Null);
        let source = "One[^a] two[^b].\n\n[^a]: First.\n[^b]: Second.";
        assert!(crate::edit::set_document_markdown(&mut project, "d", source).is_ok());
        project
    }

    fn document(project: &JsValue) -> JsObject {
        project
            .get("documents")
            .and_then(JsValue::as_array)
            .and_then(|d| d.first())
            .and_then(JsValue::as_object)
            .cloned()
            .unwrap_or_default()
    }

    /// What the open sync folder keeps of the scene: its round trip through Markdown.
    fn round_trip(project: &JsValue) -> (String, String) {
        let normalized = normalize_document(&document(project));
        let field = |k: &str| normalized.get(k).map(js::stringify).unwrap_or_default();
        (field("content"), field("footnotes"))
    }

    /// Every footnote change drops the Markdown source, so the open folder's round trip
    /// (`normalizeDocument`) keeps the change instead of bringing the old footnotes back.
    #[test]
    fn a_footnote_change_survives_the_open_folder_round_trip() {
        let mut p = project();
        assert!(document(&p).get("markdownSource").is_some());
        assert!(update_definition(&mut p, "d", "a", "Edited <em>first</em>.").is_ok());
        assert!(document(&p).get("markdownSource").is_none());
        assert!(round_trip(&p).1.contains("Edited <em>first</em>."));

        let mut p = project();
        assert!(remove(&mut p, "d", "b").is_ok());
        let (content, notes) = round_trip(&p);
        assert!(!content.contains(r#"data-footnote-id=\"b\""#) && !notes.contains("Second"));

        let mut p = project();
        assert!(add_definition(&mut p, "d", "c").is_ok());
        assert!(document(&p).get("markdownSource").is_none());

        // An orphan pruned: `b`'s marker gone from the content by a rich edit first.
        let mut p = project();
        let content = document(&p)
            .get("content")
            .and_then(JsValue::as_js_string)
            .map(|c| c.to_string_lossy())
            .unwrap_or_default();
        let without_b = super::remove_refs(&content, "b");
        assert!(crate::edit::set_document_content(&mut p, "d", &without_b).is_ok());
        assert!(prune_orphans(&mut p, "d").is_ok());
        assert!(!round_trip(&p).1.contains("Second"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_definition, create_footnote_id, marker_html, numbered, orphans, prune_orphans, remove,
        remove_refs, update_definition,
    };
    use crate::js::{parse, stringify};
    use crate::project::Footnote;

    #[test]
    fn ids_markers_numbers_and_orphans() {
        let mut values = [0.0, 0.5, 0.999_999_999_9, 0.1, 0.2, 0.3].into_iter();
        assert_eq!(
            create_footnote_id(|| values.next().unwrap_or(0.0)),
            "fn-0iz37a"
        );
        let html = format!(
            "<p>a{}b{}c{}</p>",
            marker_html("fn-b"),
            marker_html("fn-a"),
            marker_html("fn-b")
        );
        let defs = [
            Footnote {
                id: "fn-a".into(),
                html: "A".into(),
            },
            Footnote {
                id: "fn-z".into(),
                html: "Z".into(),
            },
        ];
        let got: Vec<_> = numbered(&html, &defs)
            .into_iter()
            .map(|n| (n.id, n.number, n.html))
            .collect();
        assert_eq!(
            got,
            [
                ("fn-b".into(), 1, None),
                ("fn-a".into(), 2, Some("A".into()))
            ]
        );
        assert_eq!(
            orphans(&html, &defs)
                .iter()
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>(),
            ["fn-z"]
        );
        assert_eq!(
            remove_refs(&html, "fn-b"),
            format!("<p>ab{}c</p>", marker_html("fn-a"))
        );
    }

    #[test]
    fn definitions_are_added_updated_removed_and_pruned() {
        let content = format!("<p>One{}</p>", marker_html("fn-a"));
        let mut p = parse(&format!(
            r#"{{"documents":[{{"id":"d","content":{}}}]}}"#,
            crate::js::stringify(&crate::js::JsValue::str(&content))
        ))
        .expect("json");
        add_definition(&mut p, "d", "fn-a").expect("add");
        add_definition(&mut p, "d", "fn-b").expect("add");
        assert!(
            add_definition(&mut p, "d", "fn-a").is_err(),
            "ids are unique"
        );
        update_definition(&mut p, "d", "fn-a", "<b>Bold</b><script>x</script>").expect("update");
        let s = stringify(&p);
        // Sanitized as inline HTML (script content dropped: see `html::sanitize`).
        assert!(
            s.contains(
                r#""footnotes":[{"id":"fn-a","html":"<b>Bold</b>"},{"id":"fn-b","html":""}]"#
            ),
            "{s}"
        );
        prune_orphans(&mut p, "d").expect("prune");
        assert!(!stringify(&p).contains("fn-b"), "the orphan is gone");
        remove(&mut p, "d", "fn-a").expect("remove");
        assert_eq!(
            stringify(&p),
            r#"{"documents":[{"id":"d","content":"<p>One</p>"}]}"#
        );
    }
}
