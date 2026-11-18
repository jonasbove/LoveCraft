//! The parts of a book the preview navigates (`listManuscriptSections` in ProseCraft's
//! `src/lib/publish/sections.ts`), in reading order.

use super::{CompiledBook, MatterPage, PublishOptions};

/// `ManuscriptSectionGroup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Front,
    Chapters,
    Back,
}

impl Group {
    /// The navigator's heading for the group.
    pub fn label(self) -> &'static str {
        match self {
            Self::Front => "Front Matter",
            Self::Chapters => "Chapters",
            Self::Back => "Back Matter",
        }
    }
}

/// `ManuscriptSection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub group: Group,
    /// `ManuscriptSectionKind`: "cover", "title-page", "copyright-page", "dedication",
    /// "contents", "front-matter", "chapter", "back-matter" or "bibliography".
    pub kind: &'static str,
    pub label: String,
}

/// `matterPageLabel`.
pub fn matter_page_label(page: &MatterPage) -> String {
    let title = page.title.trim();
    if !title.is_empty() {
        title.to_owned()
    } else if page.blank {
        "Blank page".to_owned()
    } else {
        "Untitled page".to_owned()
    }
}

/// `listManuscriptSections`.
pub fn list(book: &CompiledBook, options: &PublishOptions) -> Vec<Section> {
    let mut out = Vec::new();
    let mut push = |id: String, group, kind, label: String| {
        out.push(Section {
            id,
            group,
            kind,
            label,
        })
    };
    if options.cover.mode != "none" && book.cover.is_some() {
        push("front-cover".into(), Group::Front, "cover", "Cover".into());
    }
    if options.include_title_page {
        push(
            "front-title-page".into(),
            Group::Front,
            "title-page",
            "Title Page".into(),
        );
    }
    if options.include_copyright_page && !book.copyright.is_empty() {
        push(
            "front-copyright".into(),
            Group::Front,
            "copyright-page",
            "Copyright".into(),
        );
    }
    if !book.dedication.is_empty() {
        push(
            "front-dedication".into(),
            Group::Front,
            "dedication",
            "Dedication".into(),
        );
    }
    if options.include_table_of_contents && !book.chapters.is_empty() {
        push(
            "front-contents".into(),
            Group::Front,
            "contents",
            "Contents".into(),
        );
    }
    for page in &book.front_matter {
        push(
            format!("front-matter-{}", page.id),
            Group::Front,
            "front-matter",
            matter_page_label(page),
        );
    }
    for chapter in &book.chapters {
        push(
            format!("chapter-{}", chapter.id),
            Group::Chapters,
            "chapter",
            chapter.label.clone(),
        );
    }
    for page in &book.back_matter {
        push(
            format!("back-matter-{}", page.id),
            Group::Back,
            "back-matter",
            matter_page_label(page),
        );
    }
    if crate::citations::bibliography::has_book_bibliography(book, options) {
        let locale = options.citation_locale.as_str();
        push(
            "back-bibliography".into(),
            Group::Back,
            "bibliography",
            crate::citations::bibliography::bibliography_heading(locale).into(),
        );
    }
    out
}
