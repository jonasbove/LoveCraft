//! Read-only views of a stored project, for the binder, outline and sidebars.
//!
//! The stored project stays a [`JsValue`] (so nothing is lost); these structs copy out the
//! fields the interface shows. The derivations follow ProseCraft `src/routes/+page.svelte`
//! (upstream `7dae544`): `liveChapters`, `scenesInChapter`, `researchTreeItems`,
//! `projectWordCount`, `chapterWordCounts`.

use crate::html::html_word_count;
use crate::js::{JsString, JsValue};

/// Deepest Wiki folder nesting shown; deeper (or cyclic) folders are not expanded further.
pub const MAX_FOLDER_DEPTH: usize = 32;

/// Spaces that older releases used before everything moved into the Wiki (`wiki-migration.ts`).
const WIKI_SPACES: &[&str] = &["Wiki", "Characters", "Places", "Research", "Notes"];

fn text(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

fn optional_text(value: &JsValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .filter(|t| !t.is_empty())
}

fn number(value: &JsValue, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(JsValue::as_number)
        .filter(|n| n.is_finite())
}

fn flag(value: &JsValue, key: &str) -> bool {
    matches!(value.get(key), Some(JsValue::Bool(true)))
}

fn list<'a>(value: &'a JsValue, key: &str) -> &'a [JsValue] {
    value
        .get(key)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    Novel,
    Screenplay,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub summary: Option<String>,
    pub trashed: bool,
    pub label_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub chapter_id: Option<String>,
    pub space: Option<String>,
    pub folder_id: Option<String>,
    pub trashed: bool,
    pub label_id: Option<String>,
    pub screenplay: bool,
    /// Sanitized HTML, as stored.
    pub content: String,
    pub footnotes: Vec<Footnote>,
    /// The Markdown last written in the Markdown editor, while no rich edit has replaced it.
    pub markdown_source: Option<String>,
}

impl Document {
    pub fn is_wiki(&self) -> bool {
        self.chapter_id.is_none()
            && self
                .space
                .as_deref()
                .is_some_and(|space| WIKI_SPACES.contains(&space))
    }

    pub fn word_count(&self) -> usize {
        html_word_count(&self.content)
    }

    /// What the Markdown editor shows: the Markdown last written there, else the content (and
    /// its footnotes) as Markdown.
    pub fn markdown(&self) -> String {
        match &self.markdown_source {
            Some(source) => source.clone(),
            None => {
                let footnotes: Vec<crate::markdown::FootnoteDef> = self
                    .footnotes
                    .iter()
                    .map(|f| crate::markdown::FootnoteDef {
                        id: f.id.clone(),
                        html: f.html.clone(),
                    })
                    .collect();
                crate::markdown::html_to_markdown(&self.content, &footnotes)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Footnote {
    pub id: String,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
}

/// A file or web link kept in the Wiki (`ResearchItem`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResearchItem {
    pub id: String,
    pub title: String,
    pub folder_id: Option<String>,
    pub link: bool,
    /// The file or link as ProseCraft keeps it on load (`normalizeResearchItems`).
    pub item: crate::research::Item,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WikiItem {
    pub id: String,
    pub title: String,
    pub depth: usize,
    pub kind: WikiItemKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WikiItemKind {
    Folder,
    Page,
    File,
    Link,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub id: String,
    pub document_id: String,
    pub quote: String,
    /// The text around the quote, to find it again after edits.
    pub prefix: String,
    pub suffix: String,
    pub body: String,
    pub author: String,
    pub created_at: f64,
    pub resolved: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub id: String,
    pub document_id: String,
    pub created_at: f64,
    pub title: String,
    pub note: Option<String>,
    pub before_restore: bool,
    /// The kept HTML.
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineNode {
    pub id: String,
    pub title: String,
    pub date: String,
    pub time: String,
    pub description: String,
}

/// One project, as the interface shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub author: String,
    pub kind: ProjectKind,
    pub trashed: bool,
    pub project_goal: f64,
    pub chapters: Vec<Chapter>,
    pub documents: Vec<Document>,
    pub folders: Vec<Folder>,
    pub research_items: Vec<ResearchItem>,
    pub labels: Vec<Label>,
    pub comments: Vec<Comment>,
    pub snapshots: Vec<Snapshot>,
    pub timeline: Vec<TimelineNode>,
    pub storyboard_cards: usize,
    /// Per-chapter word goals (`chapterGoals`), by chapter id.
    pub chapter_goals: Vec<(String, f64)>,
    /// Writing activity by day (`activity`): `YYYY-MM-DD`, edits, words added.
    pub activity: Vec<Activity>,
    /// The citation library (`citations`), normalized as ProseCraft normalizes it on load.
    pub citations: Vec<JsValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub date: String,
    pub edits: f64,
    pub words_added: f64,
}

impl Project {
    /// The view of a stored project (`isStoredProject` already checked by the caller).
    pub fn from_stored(stored: &JsValue) -> Option<Self> {
        let mut view = Self::read(stored)?;
        // ProseCraft shows a Wiki page or file whose folder no longer exists at the top level
        // (`loadProject`, `normalizeResearchItems`). Only the view changes; the stored data is
        // kept as it is.
        let folders: std::collections::HashSet<String> =
            view.folders.iter().map(|f| f.id.clone()).collect();
        for document in view
            .documents
            .iter_mut()
            .filter(|d| d.space.as_deref() == Some("Wiki"))
        {
            if document
                .folder_id
                .as_ref()
                .is_some_and(|id| !folders.contains(id))
            {
                document.folder_id = None;
            }
        }
        for item in &mut view.research_items {
            if item
                .folder_id
                .as_ref()
                .is_some_and(|id| !folders.contains(id))
            {
                item.folder_id = None;
            }
        }
        Some(view)
    }

    fn read(stored: &JsValue) -> Option<Self> {
        let project = stored.get("project")?;
        Some(Self {
            id: text(project, "id"),
            name: text(project, "name"),
            author: text(project, "author"),
            kind: if text(project, "kind") == "screenplay" {
                ProjectKind::Screenplay
            } else {
                ProjectKind::Novel
            },
            trashed: flag(project, "trashed"),
            project_goal: number(project, "projectGoal").unwrap_or(0.0),
            chapters: list(project, "chapters")
                .iter()
                .map(|c| Chapter {
                    id: text(c, "id"),
                    title: text(c, "title"),
                    summary: optional_text(c, "summary"),
                    trashed: flag(c, "trashed"),
                    label_id: optional_text(c, "labelId"),
                })
                .collect(),
            documents: list(project, "documents")
                .iter()
                .map(|d| Document {
                    id: text(d, "id"),
                    title: text(d, "title"),
                    chapter_id: optional_text(d, "chapterId"),
                    space: optional_text(d, "space"),
                    folder_id: optional_text(d, "folderId"),
                    trashed: flag(d, "trashed"),
                    label_id: optional_text(d, "labelId"),
                    screenplay: text(d, "format") == "screenplay",
                    content: text(d, "content"),
                    footnotes: list(d, "footnotes")
                        .iter()
                        .map(|f| Footnote {
                            id: text(f, "id"),
                            html: text(f, "html"),
                        })
                        .collect(),
                    // `??`: an empty source is still the source.
                    markdown_source: d
                        .get("markdownSource")
                        .and_then(JsValue::as_js_string)
                        .map(JsString::to_string_lossy),
                })
                .collect(),
            folders: list(project, "researchFolders")
                .iter()
                .chain(list(project, "noteFolders"))
                .map(|f| Folder {
                    id: text(f, "id"),
                    title: text(f, "title"),
                    parent_id: optional_text(f, "parentId"),
                })
                .collect(),
            research_items: {
                let folders: Vec<String> = list(project, "researchFolders")
                    .iter()
                    .map(|f| text(f, "id"))
                    .collect();
                crate::research::normalize_items(project.get("researchItems"), &folders)
                    .into_iter()
                    .map(|(item, folder_id, _)| {
                        let (id, link) = match &item {
                            crate::research::Item::File { id, .. } => (id.clone(), false),
                            crate::research::Item::Link { id, .. } => (id.clone(), true),
                        };
                        ResearchItem {
                            id,
                            title: item.title().to_owned(),
                            folder_id,
                            link,
                            item,
                        }
                    })
                    .collect()
            },
            labels: crate::labels::project_labels(project),
            comments: list(project, "comments")
                .iter()
                .map(|c| Comment {
                    id: text(c, "id"),
                    document_id: text(c, "documentId"),
                    quote: text(c, "quote"),
                    prefix: text(c, "prefix"),
                    suffix: text(c, "suffix"),
                    body: text(c, "body"),
                    author: text(c, "author"),
                    created_at: number(c, "createdAt").unwrap_or(0.0),
                    resolved: flag(c, "resolved"),
                })
                .collect(),
            snapshots: list(project, "snapshots")
                .iter()
                .map(|s| Snapshot {
                    id: text(s, "id"),
                    document_id: text(s, "documentId"),
                    created_at: number(s, "createdAt").unwrap_or(0.0),
                    title: text(s, "title"),
                    note: optional_text(s, "note"),
                    before_restore: text(s, "reason") == "before-restore",
                    content: text(s, "content"),
                })
                .collect(),
            timeline: list(project, "timelineNodes")
                .iter()
                .map(|t| TimelineNode {
                    id: text(t, "id"),
                    title: text(t, "title"),
                    date: text(t, "date"),
                    time: text(t, "time"),
                    description: text(t, "description"),
                })
                .collect(),
            storyboard_cards: list(project, "storyboardCards").len(),
            chapter_goals: project
                .get("chapterGoals")
                .and_then(JsValue::as_object)
                .map(|goals| {
                    goals
                        .iter()
                        .filter_map(|(id, n)| {
                            n.as_number()
                                .filter(|n| n.is_finite())
                                .map(|n| (id.to_string_lossy(), n))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            activity: project
                .get("activity")
                .and_then(JsValue::as_object)
                .map(|days| {
                    days.iter()
                        .map(|(date, day)| Activity {
                            date: date.to_string_lossy(),
                            edits: number(day, "edits").unwrap_or(0.0),
                            words_added: number(day, "wordsAdded").unwrap_or(0.0),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            citations: crate::csl::normalize_csl_items(
                project.get("citations").unwrap_or(&JsValue::Null),
            ),
        })
    }

    /// `liveChapters`.
    pub fn live_chapters(&self) -> impl Iterator<Item = &Chapter> {
        self.chapters.iter().filter(|c| !c.trashed)
    }

    /// `scenesInChapter`.
    pub fn scenes_in_chapter<'a>(
        &'a self,
        chapter_id: &'a str,
    ) -> impl Iterator<Item = &'a Document> + 'a {
        self.documents
            .iter()
            .filter(move |d| d.chapter_id.as_deref() == Some(chapter_id) && !d.trashed)
    }

    pub fn document(&self, id: &str) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    pub fn chapter_goal(&self, chapter_id: &str) -> Option<f64> {
        self.chapter_goals
            .iter()
            .find(|(id, _)| id == chapter_id)
            .map(|(_, n)| *n)
    }

    pub fn label(&self, id: &str) -> Option<&Label> {
        self.labels.iter().find(|l| l.id == id)
    }

    /// `projectWordCount`: words in every live scene.
    pub fn word_count(&self) -> usize {
        self.documents
            .iter()
            .filter(|d| d.chapter_id.is_some() && !d.trashed)
            .map(Document::word_count)
            .sum()
    }

    /// Words in a chapter's live scenes.
    pub fn chapter_word_count(&self, chapter_id: &str) -> usize {
        self.scenes_in_chapter(chapter_id)
            .map(Document::word_count)
            .sum()
    }

    /// `researchTreeItems`: folders depth-first, each followed by its subfolders, pages and
    /// files; then the pages and files outside any folder.
    pub fn wiki_tree(&self) -> Vec<WikiItem> {
        let mut items = Vec::new();
        self.append_level(None, 0, &mut items);
        for page in self.wiki_pages(None) {
            items.push(WikiItem {
                id: page.id.clone(),
                title: page.title.clone(),
                depth: 0,
                kind: WikiItemKind::Page,
            });
        }
        items.extend(self.research_in(None, 0));
        items
    }

    fn wiki_pages<'a>(
        &'a self,
        folder: Option<&'a str>,
    ) -> impl Iterator<Item = &'a Document> + 'a {
        self.documents
            .iter()
            .filter(move |d| d.is_wiki() && !d.trashed && d.folder_id.as_deref() == folder)
    }

    fn research_in(&self, folder: Option<&str>, depth: usize) -> Vec<WikiItem> {
        self.research_items
            .iter()
            .filter(|item| item.folder_id.as_deref() == folder)
            .map(|item| WikiItem {
                id: item.id.clone(),
                title: item.title.clone(),
                depth,
                kind: if item.link {
                    WikiItemKind::Link
                } else {
                    WikiItemKind::File
                },
            })
            .collect()
    }

    fn append_level(&self, parent: Option<&str>, depth: usize, items: &mut Vec<WikiItem>) {
        if depth >= MAX_FOLDER_DEPTH {
            return;
        }
        for folder in self
            .folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == parent)
        {
            items.push(WikiItem {
                id: folder.id.clone(),
                title: folder.title.clone(),
                depth,
                kind: WikiItemKind::Folder,
            });
            self.append_level(Some(&folder.id), depth + 1, items);
            for page in self.wiki_pages(Some(&folder.id)) {
                items.push(WikiItem {
                    id: page.id.clone(),
                    title: page.title.clone(),
                    depth: depth + 1,
                    kind: WikiItemKind::Page,
                });
            }
            items.extend(self.research_in(Some(&folder.id), depth + 1));
        }
    }

    /// What the Trash group lists: trashed chapters and trashed documents that didn't go with a
    /// trashed chapter.
    pub fn trash(&self) -> (Vec<&Chapter>, Vec<&Document>) {
        let chapters = self.chapters.iter().filter(|c| c.trashed).collect();
        let documents = self.documents.iter().filter(|d| d.trashed).collect();
        (chapters, documents)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_FOLDER_DEPTH, Project, WikiItemKind};
    use crate::js::parse;

    fn project(extra: &str) -> Project {
        let text = format!(
            r#"{{"project":{{"id":"p","name":"N","author":"A","documents":[
                {{"id":"s1","title":"One","chapterId":"c1","content":"<p>three little words</p>"}},
                {{"id":"s2","title":"Gone","chapterId":"c1","trashed":true,"content":"<p>x</p>"}},
                {{"id":"w1","title":"Mara","space":"Wiki","folderId":"f1","content":""}},
                {{"id":"w2","title":"Loose","space":"Characters","content":""}}
              ],"chapters":[{{"id":"c1","title":"C1"}},{{"id":"c2","title":"Old","trashed":true}}],
              "activeDocumentId":"s1","projectGoal":1000,"chapterGoals":{{}},"activity":{{}}{extra}}}}}"#
        );
        parse(&text)
            .ok()
            .and_then(|v| Project::from_stored(&v))
            .unwrap_or_else(|| panic!("fixture"))
    }

    #[test]
    fn derives_binder_contents() {
        let p = project(r#","researchFolders":[{"id":"f1","title":"People","parentId":null}]"#);
        assert_eq!(p.live_chapters().count(), 1);
        assert_eq!(p.scenes_in_chapter("c1").count(), 1);
        assert_eq!(p.word_count(), 3);
        let tree: Vec<(String, usize, WikiItemKind)> = p
            .wiki_tree()
            .into_iter()
            .map(|i| (i.title, i.depth, i.kind))
            .collect();
        assert_eq!(
            tree,
            [
                ("People".into(), 0, WikiItemKind::Folder),
                ("Mara".into(), 1, WikiItemKind::Page),
                ("Loose".into(), 0, WikiItemKind::Page)
            ]
        );
    }

    #[test]
    fn cyclic_or_deep_folders_do_not_hang() {
        let p = project(
            r#","researchFolders":[{"id":"a","title":"A","parentId":"b"},{"id":"b","title":"B","parentId":"a"}]"#,
        );
        assert!(p.wiki_tree().iter().all(|i| i.kind != WikiItemKind::Folder));
        let chain: Vec<String> = (0..100)
            .map(|i| {
                format!(
                    r#"{{"id":"f{i}","title":"F{i}","parentId":{}}}"#,
                    if i == 0 {
                        "null".into()
                    } else {
                        format!("\"f{}\"", i - 1)
                    }
                )
            })
            .collect();
        let deep = project(&format!(r#","researchFolders":[{}]"#, chain.join(",")));
        let folders = deep
            .wiki_tree()
            .into_iter()
            .filter(|i| i.kind == WikiItemKind::Folder)
            .count();
        assert_eq!(folders, MAX_FOLDER_DEPTH);
    }
}
