//! Citation markers as the exporters write them (`resolveBookCitations`). Formatting sources
//! (CSL styles, the bibliography) isn't ported, so every marker is written as ProseCraft
//! writes a source it can't find: `[@key; @other]`. Invalid markers are removed.

use crate::cite::{CITE_REF_CLASS, parse_cite_data};
use crate::html::{Node, parse_fragment, serialize};

use super::CompiledBook;

fn resolve_nodes(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while i < nodes.len() {
        if let Node::Element(e) = &mut nodes[i] {
            if e.name == "span" && e.has_class(CITE_REF_CLASS) {
                match parse_cite_data(e.attr("data-cites")) {
                    Some(cites) => {
                        let keys: Vec<String> =
                            cites.iter().map(|c| format!("@{}", c.id)).collect();
                        nodes[i] = Node::Text(format!("[{}]", keys.join("; ")));
                        i += 1;
                    }
                    None => {
                        nodes.remove(i);
                    }
                }
                continue;
            }
            resolve_nodes(&mut e.children);
        }
        i += 1;
    }
}

/// `resolveInlineCitationsInHtml` with no sources: the HTML with its citations as text.
pub fn resolve_citations_in_html(html: &str) -> String {
    if !html.contains(CITE_REF_CLASS) {
        return html.to_owned();
    }
    let mut nodes = parse_fragment(html);
    resolve_nodes(&mut nodes);
    serialize(&nodes)
}

/// The book with citations resolved in every scene and footnote.
pub fn resolve_book_citations(book: &CompiledBook) -> CompiledBook {
    let mut book = book.clone();
    for scene in book.chapters.iter_mut().flat_map(|c| c.scenes.iter_mut()) {
        scene.html = resolve_citations_in_html(&scene.html);
        for note in &mut scene.footnotes {
            note.html = resolve_citations_in_html(&note.html);
        }
    }
    book
}
