//! `compileBook`: every live scene in chapter order, with its footnotes numbered.

use std::collections::HashMap;

use crate::edit::chapter_label;
use crate::html::sanitize::is_valid_footnote_id;
use crate::html::{
    Node, PlainTextOptions, RichText, count_words, html_to_plain_text, is_js_whitespace,
    parse_fragment, sanitize_html, sanitize_inline_html,
};
use crate::project::Project;

use super::PublishOptions;

/// A front- or back-matter page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatterPage {
    pub id: String,
    pub title: String,
    pub body: String,
    pub blank: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledFootnote {
    pub id: String,
    pub html: String,
    /// Its number in the book (continuous, or per chapter).
    pub number: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledScene {
    pub id: String,
    pub title: String,
    pub html: String,
    pub words: usize,
    pub footnotes: Vec<CompiledFootnote>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledChapter {
    pub id: String,
    /// "Chapter N" renumbered by place, else the title.
    pub label: String,
    pub title: String,
    pub scenes: Vec<CompiledScene>,
    pub words: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledBook {
    pub title: String,
    pub subtitle: String,
    pub author: String,
    pub copyright: String,
    pub dedication: String,
    pub front_matter: Vec<MatterPage>,
    pub back_matter: Vec<MatterPage>,
    pub chapters: Vec<CompiledChapter>,
    pub words: usize,
    /// The project's citation library, and the bibliography of the sources cited (when the
    /// options want one).
    pub citations: Vec<crate::js::JsValue>,
    pub bibliography: Option<crate::citations::bibliography::BookBibliography>,
    /// The cover, attached at export time (`resolveCoverImage`): `compile_book` leaves it
    /// unset.
    pub cover: Option<super::cover::CompiledCover>,
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `compileMatterPages`: trimmed, and empty pages left out unless they are blank pages.
fn compile_matter(pages: &[MatterPage]) -> Vec<MatterPage> {
    pages
        .iter()
        .map(|p| MatterPage {
            id: p.id.clone(),
            title: js_trim(&p.title).to_owned(),
            body: js_trim(&p.body).to_owned(),
            blank: p.blank,
        })
        .filter(|p| !p.title.is_empty() || !p.body.is_empty() || p.blank)
        .collect()
}

fn collect_footnote_refs(nodes: &[Node], ids: &mut Vec<String>) {
    for node in nodes {
        if let Node::Element(e) = node {
            if e.name == "sup" && e.has_class("footnote-ref") {
                if let Some(id) = e
                    .attr("data-footnote-id")
                    .filter(|id| !ids.iter().any(|i| i == id))
                {
                    ids.push(id.to_owned());
                }
            } else {
                collect_footnote_refs(&e.children, ids);
            }
        }
    }
}

fn has_rich_or_image(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element(e) => {
            let rich = [
                "math-inline",
                "math-block",
                "mermaid-diagram",
                "calc-worksheet",
            ]
            .iter()
            .any(|c| e.has_class(c));
            let image = e.name == "img"
                && e.attr("src").is_some_and(|s| {
                    [
                        "data:image/png;base64,",
                        "data:image/jpeg;base64,",
                        "data:image/webp;base64,",
                    ]
                    .iter()
                    .any(|p| s.starts_with(p))
                });
            rich || image || has_rich_or_image(&e.children)
        }
        Node::Text(_) => false,
    })
}

/// The words ProseCraft counts for a scene: no equations or diagrams, no list markers.
fn countable_words(html: &str) -> usize {
    let text = html_to_plain_text(
        html,
        &PlainTextOptions {
            rich: RichText::Omit,
            ..Default::default()
        },
    );
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let marker = line.strip_prefix("• ").or_else(|| {
                let digits = line.chars().take_while(char::is_ascii_digit).count();
                (digits > 0)
                    .then(|| line.get(digits..))
                    .flatten()
                    .and_then(|rest| rest.strip_prefix(". "))
            });
            marker.map_or_else(
                || line.to_owned(),
                |rest| rest.trim_start_matches(is_js_whitespace).to_owned(),
            )
        })
        .collect();
    count_words(&lines.join("\n"))
}

/// `compileBook` for a project's live chapters, as ProseCraft's Publish view calls it.
pub fn compile_book(project: &Project, options: &PublishOptions) -> CompiledBook {
    let live: Vec<_> = project.live_chapters().collect();
    let mut chapters = Vec::new();
    let mut next_number = 0;
    for (index, chapter) in live.iter().enumerate() {
        if options.footnotes_restart_per_chapter {
            next_number = 0;
        }
        let mut scenes = Vec::new();
        for document in &project.documents {
            let is_wiki = document.space.as_deref().is_some_and(|s| !s.is_empty());
            if is_wiki
                || document.chapter_id.as_deref() != Some(chapter.id.as_str())
                || document.trashed
            {
                continue;
            }
            // As ProseCraft: the scene sanitized, its footnotes valid, unique and inline.
            let html = sanitize_html(&document.content);
            let nodes = parse_fragment(&html);
            let text = html_to_plain_text(&html, &PlainTextOptions::default());
            if js_trim(&text).is_empty() && !has_rich_or_image(&nodes) {
                continue;
            }
            let mut ids = Vec::new();
            collect_footnote_refs(&nodes, &mut ids);
            let mut definitions: HashMap<&str, String> = HashMap::new();
            for f in &document.footnotes {
                if is_valid_footnote_id(&f.id) && !definitions.contains_key(f.id.as_str()) {
                    definitions.insert(f.id.as_str(), sanitize_inline_html(&f.html));
                }
            }
            let footnotes = ids
                .into_iter()
                .map(|id| {
                    next_number += 1;
                    let html = definitions.get(id.as_str()).cloned().unwrap_or_default();
                    CompiledFootnote {
                        id,
                        html,
                        number: next_number,
                    }
                })
                .collect();
            scenes.push(CompiledScene {
                id: document.id.clone(),
                title: if document.title.is_empty() {
                    "Untitled scene".to_owned()
                } else {
                    document.title.clone()
                },
                words: countable_words(&html),
                html,
                footnotes,
            });
        }
        if scenes.is_empty() {
            continue;
        }
        let titles: Vec<&str> = live
            .iter()
            .take(index + 1)
            .map(|c| c.title.as_str())
            .collect();
        chapters.push(CompiledChapter {
            id: chapter.id.clone(),
            label: chapter_label(&chapter.title, &titles),
            title: chapter.title.clone(),
            words: scenes.iter().map(|s: &CompiledScene| s.words).sum(),
            scenes,
        });
    }
    let title = js_trim(&project.name);
    CompiledBook {
        title: if title.is_empty() {
            "Untitled".to_owned()
        } else {
            title.to_owned()
        },
        subtitle: js_trim(&options.subtitle).to_owned(),
        author: js_trim(&project.author).to_owned(),
        copyright: js_trim(&options.copyright).to_owned(),
        dedication: js_trim(&options.dedication).to_owned(),
        front_matter: compile_matter(&options.front_matter),
        back_matter: compile_matter(&options.back_matter),
        words: chapters.iter().map(|c| c.words).sum(),
        bibliography: (!project.citations.is_empty() && options.include_bibliography)
            .then(|| {
                crate::citations::bibliography::build_book_bibliography(
                    &chapters,
                    &project.citations,
                    &options.citation_style,
                    &options.citation_locale,
                )
            })
            .flatten(),
        citations: project.citations.clone(),
        chapters,
        cover: None,
    }
}

impl CompiledBook {
    /// The number of scenes.
    pub fn scenes(&self) -> usize {
        self.chapters.iter().map(|c| c.scenes.len()).sum()
    }

    /// The Publish view's page estimate: the body at the style's words per page plus a page
    /// for each cover (in PDF and DOCX), title, copyright, dedication, contents, matter and
    /// bibliography page. `None` when the style is reflowable or its words per page aren't
    /// known.
    pub fn page_estimate(&self, options: &PublishOptions) -> Option<usize> {
        let words_per_page = options.words_per_page.filter(|w| *w > 0)? as usize;
        let body = self.words.div_ceil(words_per_page).max(1);
        let contents = if options.include_table_of_contents && !self.chapters.is_empty() {
            self.chapters.len().div_ceil(24).max(1)
        } else {
            0
        };
        let front = usize::from(options.include_title_page)
            + usize::from(options.include_copyright_page && !self.copyright.is_empty())
            + usize::from(!self.dedication.is_empty())
            + contents
            + self.front_matter.len();
        let sections = super::sections::list(self, options);
        let has = |kind: &str| sections.iter().any(|s| s.kind == kind);
        let cover = usize::from(has("cover") && matches!(options.format.as_ref(), "pdf" | "docx"));
        let bibliography = usize::from(has("bibliography"));
        Some(body + cover + front + self.back_matter.len() + bibliography)
    }
}
